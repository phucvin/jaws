use boa_ast::{
    declaration::{Declaration, LexicalDeclaration, VarDeclaration, VariableList},
    expression::{
        access::PropertyAccess,
        literal::Literal,
        operator::{
            binary::{ArithmeticOp, BinaryOp},
            update::UpdateTarget,
            Assign, Binary, Unary, Update,
        },
        Call, Expression, Identifier, New,
    },
    function::{ArrowFunction, FormalParameterList, Function, FunctionBody},
    statement::{Block, Catch, Finally, If, Return, Statement, Throw, Try, WhileLoop},
    visitor::{VisitWith, Visitor},
    StatementListItem,
};
use boa_interner::{Interner, Sym, ToInternedString};
use boa_parser::{Parser, Source};
use rand::{distributions::Alphanumeric, Rng};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Write},
    ops::{ControlFlow, Deref},
};

mod wat_ast;
mod wat_template;
use wat_ast::{WatFunction, WatInstruction as W, WatModule};

fn gen_function_name(s: Option<String>) -> String {
    let r: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();

    if let Some(s) = s {
        format!("{s}-{r}")
    } else {
        format!("function-{r}")
    }
}

struct WasmTranslator {
    module: WatModule,
    function_stack: Vec<WatFunction>,
    interner: Interner,
    functions: HashMap<String, String>,
    init_code: Vec<String>,
    data_entries: Vec<String>,
    data_offset: i32,
    identifiers_map: HashMap<i32, i32>,
    current_block_number: u32,
}

impl WasmTranslator {
    fn new(interner: Interner) -> Self {
        let module = WatModule::new();
        let function = WatFunction::new("init".to_string());
        Self {
            module,
            function_stack: vec![function],
            interner,
            functions: HashMap::new(),
            init_code: Vec::new(),
            data_entries: Vec::new(),
            data_offset: 200,
            identifiers_map: HashMap::new(),
            current_block_number: 0,
        }
    }

    fn add_symbol(&mut self, identifier: Sym, value: &str) -> i32 {
        println!(
            "symbol: {}, JS: {}",
            identifier.get(),
            self.interner.resolve(identifier).unwrap()
        );
        if let Some(offset) = self.identifiers_map.get(&(identifier.get() as i32)) {
            *offset
        } else {
            let (offset, _) = self.insert_data_string(value);
            self.identifiers_map.insert(identifier.get() as i32, offset);
            offset
        }
    }
    fn add_identifier(&mut self, identifier: &Identifier) -> i32 {
        self.add_symbol(
            identifier.sym(),
            &self.interner.resolve(identifier.sym()).unwrap().to_string(),
        )
    }

    fn current_function(&mut self) -> &mut WatFunction {
        self.function_stack.last_mut().unwrap()
    }

    fn enter_function(&mut self, function: WatFunction) {
        self.function_stack.push(function);
    }

    fn exit_function(&mut self) {
        let function = self.function_stack.pop().unwrap();
        self.module.add_function(function);
    }

    fn enter_block(&mut self) {
        self.current_block_number += 1;
    }

    fn exit_block(&mut self) {
        self.current_block_number -= 1;
    }

    fn current_block_name(&self) -> String {
        format!("$block-{}", self.current_block_number)
    }

    fn translate_return(&mut self, ret: &Return) -> Box<W> {
        // println!("Return: {ret:#?}");
        let mut instructions = Vec::new();
        if let Some(target) = ret.target() {
            instructions.push(self.translate_expression(target));
        } else {
            instructions.push(W::ref_null("any"));
        }
        instructions.push(W::r#return());
        W::list(instructions)
    }

    fn translate_function_generic(
        &mut self,
        name: Option<Identifier>,
        params: &FormalParameterList,
        body: &FunctionBody,
    ) -> Box<W> {
        let function_name = gen_function_name(name.map(|i| i.to_interned_string(&self.interner)));
        let wat_function = WatFunction::new(function_name.clone());
        self.enter_function(wat_function);

        self.current_function()
            .add_param("$parentScope".to_string(), "(ref $Scope)".to_string());
        self.current_function()
            .add_param("$this".to_string(), "anyref".to_string());
        self.current_function()
            .add_param("$arguments".to_string(), "(ref null $JSArgs)".to_string());
        self.current_function().add_result("anyref".to_string());

        self.current_function()
            .locals
            .insert(("$scope".to_string(), "(ref $Scope)".to_string()));
        self.current_function()
            .add_instruction(W::call("$new_scope", vec![W::local_get("$parentScope")]));
        self.current_function()
            .add_instruction(W::local_set("$scope"));

        // set parameters on the scope
        for (i, param) in params.as_ref().iter().enumerate() {
            match param.variable().binding() {
                boa_ast::declaration::Binding::Identifier(identifier) => {
                    let offset = self.add_identifier(identifier);
                    self.current_function().add_instruction(W::call(
                        "$set_variable",
                        vec![
                            W::local_get("$scope"),
                            W::i32_const(offset),
                            W::instruction(
                                "array.get",
                                vec![
                                    W::r#type("$JSArgs"),
                                    W::local_get("$arguments"),
                                    W::i32_const(i as i32),
                                ],
                            ),
                        ],
                    ));
                }
                boa_ast::declaration::Binding::Pattern(_pattern) => todo!(),
            }
        }

        for statement in body.statements().statements() {
            match statement {
                boa_ast::StatementListItem::Statement(statement) => {
                    let res = self.translate_statement(statement);
                    self.current_function().add_instruction(res);
                }
                boa_ast::StatementListItem::Declaration(declaration) => {
                    let declaration = self.translate_declaration(declaration);
                    self.current_function().add_instruction(declaration);
                }
            }
        }

        // This is a bit dumb, but it will work for now - every $JSFunc
        // has to return a value. If we already returned this will get ignored
        // If not, ie. there is no return statement, we will return undefined
        self.current_function()
            .add_instruction(W::list(vec![W::ref_null("any"), W::r#return()]));

        self.exit_function();

        W::call(
            "$new_function".to_string(),
            vec![W::local_get("$scope"), W::ref_func(function_name)],
        )
    }

    fn translate_function(&mut self, fun: &Function) -> Box<W> {
        // println!(
        //     "translate function: {}",
        //     fun.to_interned_string(&self.interner)
        // );

        self.translate_function_generic(fun.name(), fun.parameters(), fun.body())
    }

    fn translate_lexical(&mut self, decl: &LexicalDeclaration) -> Box<W> {
        // println!(
        //     "translate lexical {}",
        //     decl.to_interned_string(&self.interner)
        // );
        match decl {
            LexicalDeclaration::Const(_variable_list) => todo!(),
            LexicalDeclaration::Let(variable_list) => self.translate_let_vars(variable_list),
        }
    }

    fn translate_let(&mut self, decl: &VarDeclaration) -> Box<W> {
        //println!("LET: {:#?}", decl.0);
        // TODO: variables behave a bit differently when it comes to hoisting
        // for now I just ignore it, but it should be fixed
        // https://developer.mozilla.org/en-US/docs/Glossary/Hoisting
        self.translate_let_vars(&decl.0)
    }

    fn translate_call(&mut self, call: &Call, get_this: Box<W>) -> Box<W> {
        // println!(
        //     "translate_call {}",
        //     call.function().to_interned_string(&self.interner)
        // );
        let function_name = call.function().to_interned_string(&self.interner);
        let mut instructions = Vec::new();

        // Add a local for arguments to the current function
        self.current_function()
            .add_local("$call_arguments", "(ref $JSArgs)");
        self.current_function().add_local("$temp_arg", "anyref");

        // Create the arguments array
        let args_count = call.args().len() as i32;
        instructions.push(W::array_new(
            "$JSArgs",
            W::ref_null("any"),
            W::i32_const(args_count),
        ));
        instructions.push(W::local_set("$call_arguments"));

        // Populate the arguments array
        for (index, arg) in call.args().iter().enumerate() {
            let arg_instruction = self.translate_expression(arg);
            instructions.push(W::list(vec![
                arg_instruction,
                W::local_set("$temp_arg"),
                W::instruction(
                    "array.set",
                    vec![
                        W::r#type("$JSArgs"),
                        W::local_get("$call_arguments"),
                        W::i32_const(index as i32),
                        W::local_get("$temp_arg"),
                    ],
                ),
            ]));
        }

        if function_name == "console.log" {
            instructions.push(W::call(
                "$log",
                vec![W::local_get("$call_arguments".to_string())],
            ));
        } else {
            // Translate the function expression
            self.current_function()
                .locals
                .insert(("$function".to_string(), "anyref".to_string()));
            instructions.push(self.translate_expression(call.function()));
            instructions.push(W::local_set("$function"));

            // Call the function
            instructions.push(W::call(
                "$call_function",
                vec![
                    W::local_get("$scope"),
                    W::local_get("$function"),
                    get_this,
                    W::local_get("$call_arguments"),
                ],
            ));
        }

        W::list(instructions)
    }

    fn translate_let_vars(&mut self, variable_list: &VariableList) -> Box<W> {
        use boa_ast::declaration::Binding;

        self.current_function()
            .locals
            .insert(("$var".to_string(), "anyref".to_string()));

        let mut instructions = Vec::new();
        for var in variable_list.as_ref() {
            if let Some(expression) = var.init() {
                match var.binding() {
                    Binding::Identifier(identifier) => {
                        let offset = self.add_identifier(&identifier);

                        instructions.push(self.translate_expression(expression));
                        instructions.push(W::local_set("$var"));

                        instructions.push(W::local_get("$scope"));
                        instructions.push(W::i32_const(offset));
                        instructions.push(W::local_get("$var"));
                        instructions.push(W::call("$set_variable".to_string(), vec![]));
                    }
                    Binding::Pattern(_pattern) => todo!(),
                }
            }
        }

        W::list(instructions)
    }

    fn translate_binary(&mut self, binary: &Binary) -> Box<W> {
        use boa_ast::expression::operator::binary::RelationalOp;

        // println!("Binary: {binary:#?}");
        match binary.op() {
            BinaryOp::Arithmetic(arithmetic_op) => {
                let func = match arithmetic_op {
                    ArithmeticOp::Add => "$add",
                    ArithmeticOp::Sub => "$sub",
                    ArithmeticOp::Div => "$div",
                    ArithmeticOp::Mul => "$mul",
                    ArithmeticOp::Exp => "$exp",
                    ArithmeticOp::Mod => "$mod",
                };
                // TODO: this will probably need translating to
                // multiple lines and saving to local vars
                let lhs = self.translate_expression(binary.lhs());
                let rhs = self.translate_expression(binary.rhs());
                W::call(func.to_string(), vec![lhs, rhs])
            }
            BinaryOp::Bitwise(_bitwise_op) => todo!(),
            BinaryOp::Relational(relational_op) => {
                let func_name = match relational_op {
                    RelationalOp::Equal => todo!(),
                    RelationalOp::NotEqual => todo!(),
                    RelationalOp::StrictEqual => "$strict_equal",
                    RelationalOp::StrictNotEqual => "$strict_not_equal",
                    RelationalOp::GreaterThan => todo!(),
                    RelationalOp::GreaterThanOrEqual => "$greater_than_or_equal",
                    RelationalOp::LessThan => "$less_than",
                    RelationalOp::LessThanOrEqual => todo!(),
                    RelationalOp::In => todo!(),
                    RelationalOp::InstanceOf => todo!(),
                };
                self.current_function()
                    .locals
                    .insert(("$lhs".to_string(), "anyref".to_string()));
                self.current_function()
                    .locals
                    .insert(("$rhs".to_string(), "anyref".to_string()));

                W::list(vec![
                    self.translate_expression(binary.rhs()),
                    W::local_set("$rhs"),
                    self.translate_expression(binary.lhs()),
                    W::local_get("$rhs"),
                    W::call(func_name, vec![]),
                ])
            }
            BinaryOp::Logical(_logical_op) => todo!(),
            BinaryOp::Comma => todo!(),
        }
    }

    fn translate_identifier(&mut self, identifier: &Identifier) -> Box<W> {
        let offset = self.add_identifier(identifier);

        if identifier.to_interned_string(&self.interner) == "undefined" {
            W::ref_null("any")
        } else {
            W::call(
                "$get_variable".to_string(),
                vec![W::local_get("$scope"), W::i32_const(offset)],
            )
        }
    }

    fn translate_property_access(
        &mut self,
        property_access: &PropertyAccess,
        assign: Option<Box<W>>,
    ) -> Box<W> {
        use boa_ast::expression::access::PropertyAccessField;

        println!("Property access: {:#?}", property_access);

        match property_access {
            PropertyAccess::Simple(simple_property_access) => {
                let target = self.translate_expression(simple_property_access.target());
                match simple_property_access.field() {
                    PropertyAccessField::Const(sym) => {
                        let field = self.interner.resolve(*sym).unwrap().to_string();
                        let offset = self.add_symbol(*sym, &field);
                        // self.current_function().add_local("$target", "anyref");

                        if let Some(assign_instruction) = assign {
                            self.current_function().add_local("$temp_anyref", "anyref");
                            W::list(vec![
                                assign_instruction,
                                W::local_set("$temp_anyref"),
                                target,
                                W::i32_const(offset),
                                W::local_get("$temp_anyref"),
                                W::call("$set_property", vec![]),
                            ])
                        } else {
                            W::list(vec![
                                target,
                                W::i32_const(offset),
                                W::call("$get_property", vec![]),
                            ])
                        }
                    }
                    PropertyAccessField::Expr(_expression) => todo!(),
                }
            }
            PropertyAccess::Private(_private_property_access) => todo!(),
            PropertyAccess::Super(_super_property_access) => todo!(),
        }
    }

    fn translate_expression(&mut self, expression: &Expression) -> Box<W> {
        println!(
            "translate expression {}",
            expression.to_interned_string(&self.interner)
        );
        match expression {
            Expression::This => W::local_get("$this"),
            Expression::Identifier(identifier) => self.translate_identifier(identifier),
            Expression::Literal(literal) => self.translate_literal(literal),
            Expression::RegExpLiteral(_reg_exp_literal) => todo!(),
            Expression::ArrayLiteral(_array_literal) => todo!(),
            Expression::ObjectLiteral(_object_literal) => todo!(),
            Expression::Spread(_spread) => todo!(),
            Expression::Function(function) => self.translate_function(function),
            Expression::ArrowFunction(arrow_function) => {
                self.translate_arrow_function(arrow_function)
            }
            Expression::AsyncArrowFunction(_async_arrow_function) => todo!(),
            Expression::Generator(_generator) => todo!(),
            Expression::AsyncFunction(_async_function) => todo!(),
            Expression::AsyncGenerator(_async_generator) => todo!(),
            Expression::Class(_class) => todo!(),
            Expression::TemplateLiteral(_template_literal) => todo!(),
            Expression::PropertyAccess(property_access) => {
                self.translate_property_access(property_access, None)
            }
            Expression::New(new) => self.translate_new(new),
            // TODO: the default this value is a global object
            Expression::Call(call) => self.translate_call(call, W::ref_null("any")),
            Expression::SuperCall(_super_call) => todo!(),
            Expression::ImportCall(_import_call) => todo!(),
            Expression::Optional(_optional) => todo!(),
            Expression::TaggedTemplate(_tagged_template) => todo!(),
            Expression::NewTarget => todo!(),
            Expression::ImportMeta => todo!(),
            Expression::Assign(assign) => self.translate_assign(assign),
            Expression::Unary(unary) => self.translate_unary(unary),
            Expression::Update(update) => self.translate_update(update),
            Expression::Binary(binary) => self.translate_binary(binary),
            Expression::BinaryInPrivate(_binary_in_private) => todo!(),
            Expression::Conditional(_conditional) => todo!(),
            Expression::Await(_) => todo!(),
            Expression::Yield(_) => todo!(),
            Expression::Parenthesized(_parenthesized) => todo!(),
            _ => todo!(),
        }
    }

    fn translate_new(&mut self, new: &New) -> Box<W> {
        self.current_function()
            .add_local("$new_instance", "(ref $Object)");
        W::list(vec![
            W::call("$new_object", vec![]),
            W::local_set("$new_instance"),
            self.translate_call(new.call(), W::local_get("$new_instance")),
            W::drop(),
            // TODO: we return the created instance, but it's not always the case
            // in JS. If the returned value is an object, we should return the returned
            // value, so we need to add an if with a `ref.test` here
            W::local_get("$new_instance"),
        ])
    }

    fn translate_arrow_function(&mut self, function: &ArrowFunction) -> Box<W> {
        self.translate_function_generic(function.name(), function.parameters(), function.body())
    }

    fn translate_update(&mut self, update: &Update) -> Box<W> {
        use boa_ast::expression::operator::update::UpdateOp;
        let identifier = match update.target() {
            UpdateTarget::Identifier(identifier) => identifier,
            UpdateTarget::PropertyAccess(_property_access) => todo!(),
        };
        self.current_function()
            .locals
            .insert(("$var".to_string(), "anyref".to_string()));

        // TODO: figure out pre vs post behaviour
        let instruction = match update.op() {
            UpdateOp::IncrementPost => W::call("$increment_number", vec![]),
            UpdateOp::IncrementPre => W::call("$increment_number", vec![]),
            UpdateOp::DecrementPost => W::call("$decrement_number", vec![]),
            UpdateOp::DecrementPre => W::call("$decrement_number", vec![]),
        };
        let target = self.translate_identifier(identifier);
        let offset = self.add_identifier(identifier);
        let set_variable = W::call(
            "$set_variable".to_string(),
            vec![
                W::local_get("$scope".to_string()),
                W::i32_const(offset),
                W::local_get("$var"),
            ],
        );

        W::list(vec![
            target,
            instruction,
            W::local_set("$var"),
            set_variable,
        ])
    }

    fn translate_assign(&mut self, assign: &Assign) -> Box<W> {
        use boa_ast::expression::operator::assign::AssignOp;
        use boa_ast::expression::operator::assign::AssignTarget;

        // println!("assign: {:#?}", assign);
        match assign.op() {
            AssignOp::Assign => {
                let rhs = self.translate_expression(assign.rhs());
                match assign.lhs() {
                    AssignTarget::Identifier(identifier) => {
                        let offset = self.add_identifier(identifier);
                        // identifier.sym().get(),
                        self.current_function()
                            .locals
                            .insert(("$rhs".to_string(), "anyref".to_string()));
                        W::list(vec![
                            rhs,
                            W::local_set("$rhs"),
                            W::call(
                                "$set_variable".to_string(),
                                vec![
                                    W::local_get("$scope".to_string()),
                                    W::i32_const(offset),
                                    W::local_get("$rhs"),
                                ],
                            ),
                        ])
                    }
                    AssignTarget::Access(property_access) => {
                        self.translate_property_access(property_access, Some(rhs))
                    }
                    AssignTarget::Pattern(_pattern) => todo!(),
                }
            }
            AssignOp::Add => todo!(),
            AssignOp::Sub => todo!(),
            AssignOp::Mul => todo!(),
            AssignOp::Div => todo!(),
            AssignOp::Mod => todo!(),
            AssignOp::Exp => todo!(),
            AssignOp::And => todo!(),
            AssignOp::Or => todo!(),
            AssignOp::Xor => todo!(),
            AssignOp::Shl => todo!(),
            AssignOp::Shr => todo!(),
            AssignOp::Ushr => todo!(),
            AssignOp::BoolAnd => todo!(),
            AssignOp::BoolOr => todo!(),
            AssignOp::Coalesce => todo!(),
        }
    }

    fn translate_unary(&mut self, unary: &Unary) -> Box<W> {
        println!("unary: {:#?}", unary);

        W::empty()
    }

    fn translate_literal(&mut self, lit: &Literal) -> Box<W> {
        println!("translate_literal: {lit:#?}");
        match lit {
            Literal::Num(num) => W::call("$new_number", vec![W::f64_const(*num)]),
            Literal::String(s) => {
                let s = self.interner.resolve(*s).unwrap().to_string();
                let (offset, length) = self.insert_data_string(&s);

                W::call(
                    "$new_string",
                    vec![W::i32_const(offset), W::i32_const(length)],
                )
            }
            Literal::Int(i) => W::call("$new_number", vec![W::f64_const(*i as f64)]),
            Literal::BigInt(_big_int) => todo!(),
            Literal::Bool(b) => W::call("$new_boolean", vec![W::i32_const(if *b { 1 } else { 0 })]),
            Literal::Null => W::ref_i31(W::i32_const(2)),
            Literal::Undefined => W::ref_null("any"),
        }
    }

    fn translate_declaration(&mut self, declaration: &Declaration) -> Box<W> {
        // println!(
        //     "translate_declaration {}",
        //     declaration.to_interned_string(&self.interner)
        // );
        match declaration {
            Declaration::Function(decl) => {
                let declaration = self.translate_function(decl);
                // function declaration still needs to be added to the scope if function has a name
                if let Some(name) = decl.name() {
                    let offset = self.add_identifier(&name);
                    W::call(
                        "$set_variable".to_string(),
                        vec![W::local_get("$scope"), W::i32_const(offset), declaration],
                    )
                } else {
                    // TODO: if it's empty and not called right away I guess we can just ignore it?
                    declaration
                }
            }
            Declaration::Lexical(v) => self.translate_lexical(v),
            Declaration::Generator(_generator) => todo!(),
            Declaration::AsyncFunction(_async_function) => todo!(),
            Declaration::AsyncGenerator(_async_generator) => todo!(),
            Declaration::Class(_class) => todo!(),
        }
    }

    fn data(&self) -> String {
        self.data_entries.join("\n")
    }

    // TODO: we can save some space by checking if a string already exists
    fn insert_data_string(&mut self, s: &str) -> (i32, i32) {
        let offset = self.data_offset;
        self.data_entries
            .push(format!("(data (i32.const {offset}) \"{s}\")"));
        let len = s.len() as i32;
        self.data_offset += if len % 4 == 0 {
            len
        } else {
            // some runtimes expect all data aligned to 4 bytes
            len + (4 - len % 4)
        };

        (offset, len)
    }

    fn translate_statement(&mut self, statement: &Statement) -> Box<W> {
        match statement {
            Statement::Block(block) => self.translate_block(block),
            Statement::Var(var_declaration) => self.translate_let(var_declaration),
            Statement::Empty => todo!(),
            Statement::Expression(expression) => self.translate_expression(expression),
            Statement::If(if_statement) => self.translate_if_statement(if_statement),
            Statement::DoWhileLoop(_do_while_loop) => todo!(),
            Statement::WhileLoop(while_loop) => self.translate_while_loop(while_loop),
            Statement::ForLoop(_for_loop) => todo!(),
            Statement::ForInLoop(_for_in_loop) => todo!(),
            Statement::ForOfLoop(_for_of_loop) => todo!(),
            Statement::Switch(_switch) => todo!(),
            Statement::Continue(_) => todo!(),
            Statement::Break(_) => todo!(),
            Statement::Return(ret) => self.translate_return(ret),
            Statement::Labelled(_labelled) => todo!(),
            Statement::Throw(throw) => self.translate_throw(throw),
            Statement::Try(r#try) => self.translate_try(r#try),
            Statement::With(_with) => todo!(),
        }
    }

    fn translate_catch(&mut self, catch: Option<&Catch>, finally: Option<&Finally>) -> Box<W> {
        use boa_ast::declaration::Binding;
        let catch_instr = if let Some(catch) = catch {
            let binding_instr = if let Some(binding) = catch.parameter() {
                match binding {
                    Binding::Identifier(identifier) => {
                        self.current_function().add_local("$temp_anyref", "anyref");
                        let offset = self.add_identifier(identifier);
                        W::list(vec![
                            W::local_set("$temp_anyref"),
                            W::call(
                                "$set_variable".to_string(),
                                vec![
                                    W::local_get("$scope".to_string()),
                                    W::i32_const(offset),
                                    W::local_get("$temp_anyref"),
                                ],
                            ),
                        ])
                    }
                    Binding::Pattern(_) => todo!(),
                }
            } else {
                W::drop()
            };
            W::list(vec![binding_instr, self.translate_block(catch.block())])
        } else {
            W::empty()
        };
        let finally_instr = if let Some(finally) = finally {
            self.translate_block(finally.block())
        } else {
            W::empty()
        };
        // TODO: if catch throws an error this will not behave as it should.
        // we need to add another try inside, catch anything that happens
        // there, run finally and then rethrow
        W::catch("$JSException", W::list(vec![catch_instr, finally_instr]))
    }

    fn translate_try(&mut self, r#try: &Try) -> Box<W> {
        let block = r#try.block();
        let catch = r#try.catch();
        let finally = r#try.finally();
        let instr = self.translate_catch(catch, finally);

        W::r#try(self.translate_block(block), vec![instr], None)
    }

    fn translate_throw(&mut self, throw: &Throw) -> Box<W> {
        let target = self.translate_expression(throw.target());
        W::list(vec![target, W::throw("$JSException")])
    }

    fn translate_if_statement(&mut self, if_statement: &If) -> Box<W> {
        W::list(vec![
            self.translate_expression(if_statement.cond()),
            W::call("$cast_ref_to_i32_bool", vec![]),
            W::r#if(
                None,
                vec![self.translate_statement(if_statement.body())],
                if_statement
                    .else_node()
                    .map(|e| vec![self.translate_statement(e)]),
            ),
        ])
    }

    fn translate_while_loop(&mut self, while_loop: &WhileLoop) -> Box<W> {
        println!("condition: {:#?}", while_loop.condition());
        let condition = self.translate_expression(while_loop.condition());
        println!("while_loop: {while_loop:#?}");
        println!("condition: {condition:#?}");
        W::r#loop(
            "$while_loop".to_string(),
            vec![W::block(
                "$break",
                vec![
                    condition,
                    W::call("$cast_ref_to_i32_bool", vec![]),
                    W::i32_eqz(),
                    W::br_if("$break"),
                    self.translate_statement(while_loop.body()),
                    W::br("$while_loop"),
                ],
            )],
        )
    }

    fn translate_block(&mut self, block: &Block) -> Box<W> {
        self.enter_block();
        let block_instr = W::block(
            self.current_block_name(),
            block
                .statement_list()
                .statements()
                .iter()
                .map(|s| self.translate_statement_list_item(s))
                .collect(),
        );
        self.exit_block();

        block_instr
    }

    fn translate_statement_list_item(&mut self, statement: &StatementListItem) -> Box<W> {
        match statement {
            StatementListItem::Statement(statement) => self.translate_statement(statement),
            StatementListItem::Declaration(declaration) => self.translate_declaration(declaration),
        }
    }
}

impl<'a> Visitor<'a> for WasmTranslator {
    type BreakTy = ();

    fn visit_var_declaration(&mut self, node: &'a VarDeclaration) -> ControlFlow<Self::BreakTy> {
        println!(
            "visit_var_declaration: {}",
            node.to_interned_string(&self.interner)
        );
        let instruction = self.translate_let(node);
        self.current_function().add_instruction(instruction);
        ControlFlow::Continue(())
    }

    fn visit_declaration(&mut self, node: &Declaration) -> ControlFlow<Self::BreakTy> {
        println!(
            "visit_declaration: {}",
            node.to_interned_string(&self.interner)
        );
        let instruction = self.translate_declaration(node);
        self.current_function().add_instruction(instruction);
        ControlFlow::Continue(())
    }

    fn visit_expression(&mut self, node: &Expression) -> ControlFlow<Self::BreakTy> {
        println!(
            "visit_expression: {}",
            node.to_interned_string(&self.interner)
        );
        let instruction = self.translate_expression(node);
        self.current_function().add_instruction(instruction);
        ControlFlow::Continue(())
    }

    fn visit_statement(&mut self, node: &'a Statement) -> ControlFlow<Self::BreakTy> {
        println!(
            "visit_statement: {}",
            node.to_interned_string(&self.interner)
        );
        let instruction = self.translate_statement(node);
        self.current_function().add_instruction(instruction);
        ControlFlow::Continue(())
    }
}

fn main() -> io::Result<()> {
    let mut js_code = String::new();
    io::stdin().read_to_string(&mut js_code)?;

    let mut interner = Interner::default();
    let mut parser = Parser::new(Source::from_bytes(&js_code));
    let ast = parser.parse_script(&mut interner).unwrap();

    let mut translator = WasmTranslator::new(interner);
    ast.visit_with(&mut translator);
    // exit $init function
    translator.exit_function();

    let init = translator.module.get_function_mut("init").unwrap();
    init.locals
        .insert(("$scope".to_string(), "(ref $Scope)".to_string()));
    init.body.push_front(W::local_set("$scope".to_string()));
    init.body
        .push_front(W::call("$new_scope", vec![W::ref_null("$Scope")]));

    // Generate the full WAT module
    let module = translator.module.to_string();

    // Generate the full WAT module using the template
    let module = wat_template::generate_wat_template(
        &translator.functions,
        &module,
        &translator.data(),
        translator.data_offset + (4 - translator.data_offset % 4),
    );

    let mut f = File::create("wat/generated.wat").unwrap();
    f.write_all(module.as_bytes()).unwrap();

    println!("WAT modules generated successfully!");
    Ok(())
}
