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
    statement::{Block, If, Return, Statement, Throw, WhileLoop},
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
use wat_ast::{WatFunction, WatInstruction, WatModule};

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
        }
    }

    fn add_symbol(&mut self, identifier: Sym, value: &str) -> i32 {
        println!(
            "symbol: {}, JS: {}",
            identifier.get(),
            self.interner.resolve(identifier).unwrap().to_string()
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

    fn translate_return(&mut self, ret: &Return) -> Box<WatInstruction> {
        // println!("Return: {ret:#?}");
        let mut instructions = Vec::new();
        if let Some(target) = ret.target() {
            instructions.push(self.translate_expression(target));
        } else {
            instructions.push(WatInstruction::ref_null("any"));
        }
        instructions.push(WatInstruction::r#return());
        WatInstruction::list(instructions)
    }

    fn translate_function_generic(
        &mut self,
        name: Option<Identifier>,
        params: &FormalParameterList,
        body: &FunctionBody,
    ) -> Box<WatInstruction> {
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
            .add_instruction(WatInstruction::call(
                "$new_scope",
                vec![WatInstruction::local_get("$parentScope")],
            ));
        self.current_function()
            .add_instruction(WatInstruction::local_set("$scope"));

        // set parameters on the scope
        for (i, param) in params.as_ref().iter().enumerate() {
            match param.variable().binding() {
                boa_ast::declaration::Binding::Identifier(identifier) => {
                    let offset = self.add_identifier(identifier);
                    self.current_function()
                        .add_instruction(WatInstruction::call(
                            "$set_variable",
                            vec![
                                WatInstruction::local_get("$scope"),
                                WatInstruction::i32_const(offset),
                                WatInstruction::instruction(
                                    "array.get",
                                    vec![
                                        WatInstruction::r#type("$JSArgs"),
                                        WatInstruction::local_get("$arguments"),
                                        WatInstruction::i32_const(i as i32),
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
            .add_instruction(WatInstruction::list(vec![
                WatInstruction::ref_null("any"),
                WatInstruction::r#return(),
            ]));

        self.exit_function();

        WatInstruction::call(
            "$new_function".to_string(),
            vec![
                WatInstruction::local_get("$scope"),
                WatInstruction::ref_func(function_name),
            ],
        )
    }

    fn translate_function(&mut self, fun: &Function) -> Box<WatInstruction> {
        // println!(
        //     "translate function: {}",
        //     fun.to_interned_string(&self.interner)
        // );

        self.translate_function_generic(fun.name(), fun.parameters(), fun.body())
    }

    fn translate_lexical(&mut self, decl: &LexicalDeclaration) -> Box<WatInstruction> {
        // println!(
        //     "translate lexical {}",
        //     decl.to_interned_string(&self.interner)
        // );
        match decl {
            LexicalDeclaration::Const(_variable_list) => todo!(),
            LexicalDeclaration::Let(variable_list) => self.translate_let_vars(variable_list),
        }
    }

    fn translate_let(&mut self, decl: &VarDeclaration) -> Box<WatInstruction> {
        //println!("LET: {:#?}", decl.0);
        // TODO: variables behave a bit differently when it comes to hoisting
        // for now I just ignore it, but it should be fixed
        // https://developer.mozilla.org/en-US/docs/Glossary/Hoisting
        self.translate_let_vars(&decl.0)
    }

    fn translate_call(
        &mut self,
        call: &Call,
        get_this: Box<WatInstruction>,
    ) -> Box<WatInstruction> {
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
        instructions.push(WatInstruction::array_new(
            "$JSArgs",
            WatInstruction::ref_null("any"),
            WatInstruction::i32_const(args_count),
        ));
        instructions.push(WatInstruction::local_set("$call_arguments"));

        // Populate the arguments array
        for (index, arg) in call.args().iter().enumerate() {
            let arg_instruction = self.translate_expression(arg);
            instructions.push(WatInstruction::list(vec![
                arg_instruction,
                WatInstruction::local_set("$temp_arg"),
                WatInstruction::instruction(
                    "array.set",
                    vec![
                        WatInstruction::r#type("$JSArgs"),
                        WatInstruction::local_get("$call_arguments"),
                        WatInstruction::i32_const(index as i32),
                        WatInstruction::local_get("$temp_arg"),
                    ],
                ),
            ]));
        }

        if function_name == "console.log" {
            instructions.push(WatInstruction::call(
                "$log",
                vec![WatInstruction::local_get("$call_arguments".to_string())],
            ));
        } else {
            // Translate the function expression
            self.current_function()
                .locals
                .insert(("$function".to_string(), "anyref".to_string()));
            instructions.push(self.translate_expression(call.function()));
            instructions.push(WatInstruction::local_set("$function"));

            // Call the function
            instructions.push(WatInstruction::call(
                "$call_function",
                vec![
                    WatInstruction::local_get("$scope"),
                    WatInstruction::local_get("$function"),
                    get_this,
                    WatInstruction::local_get("$call_arguments"),
                ],
            ));
        }

        WatInstruction::list(instructions)
    }

    fn translate_let_vars(&mut self, variable_list: &VariableList) -> Box<WatInstruction> {
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
                        instructions.push(WatInstruction::local_set("$var"));

                        instructions.push(WatInstruction::local_get("$scope"));
                        instructions.push(WatInstruction::i32_const(offset));
                        instructions.push(WatInstruction::local_get("$var"));
                        instructions
                            .push(WatInstruction::call("$set_variable".to_string(), vec![]));
                    }
                    Binding::Pattern(_pattern) => todo!(),
                }
            }
        }

        WatInstruction::list(instructions)
    }

    fn translate_binary(&mut self, binary: &Binary) -> Box<WatInstruction> {
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
                WatInstruction::call(func.to_string(), vec![lhs, rhs])
            }
            BinaryOp::Bitwise(_bitwise_op) => todo!(),
            BinaryOp::Relational(relational_op) => {
                let func_name = match relational_op {
                    RelationalOp::Equal => todo!(),
                    RelationalOp::NotEqual => todo!(),
                    RelationalOp::StrictEqual => "$strict_equal",
                    RelationalOp::StrictNotEqual => todo!(),
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

                WatInstruction::list(vec![
                    self.translate_expression(binary.rhs()),
                    WatInstruction::local_set("$rhs"),
                    self.translate_expression(binary.lhs()),
                    WatInstruction::local_get("$rhs"),
                    WatInstruction::call(func_name, vec![]),
                ])
            }
            BinaryOp::Logical(_logical_op) => todo!(),
            BinaryOp::Comma => todo!(),
        }
    }

    fn translate_identifier(&mut self, identifier: &Identifier) -> Box<WatInstruction> {
        let offset = self.add_identifier(identifier);

        if identifier.to_interned_string(&self.interner) == "undefined" {
            WatInstruction::ref_null("any")
        } else {
            WatInstruction::call(
                "$get_variable".to_string(),
                vec![
                    WatInstruction::local_get("$scope"),
                    WatInstruction::i32_const(offset),
                ],
            )
        }
    }

    fn translate_property_access(
        &mut self,
        property_access: &PropertyAccess,
        assign: Option<Box<WatInstruction>>,
    ) -> Box<WatInstruction> {
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
                            WatInstruction::list(vec![
                                assign_instruction,
                                WatInstruction::local_set("$temp_anyref"),
                                target,
                                WatInstruction::i32_const(offset),
                                WatInstruction::local_get("$temp_anyref"),
                                WatInstruction::call("$set_property", vec![]),
                            ])
                        } else {
                            WatInstruction::list(vec![
                                target,
                                WatInstruction::i32_const(offset),
                                WatInstruction::call("$get_property", vec![]),
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

    fn translate_expression(&mut self, expression: &Expression) -> Box<WatInstruction> {
        println!(
            "translate expression {}",
            expression.to_interned_string(&self.interner)
        );
        match expression {
            Expression::This => WatInstruction::local_get("$this"),
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
            Expression::Call(call) => self.translate_call(call, WatInstruction::ref_null("any")),
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

    fn translate_new(&mut self, new: &New) -> Box<WatInstruction> {
        self.current_function()
            .add_local("$new_instance", "(ref $Object)");
        WatInstruction::list(vec![
            WatInstruction::call("$new_object", vec![]),
            WatInstruction::local_set("$new_instance"),
            self.translate_call(new.call(), WatInstruction::local_get("$new_instance")),
            WatInstruction::drop(),
            // TODO: we return the created instance, but it's not always the case
            // in JS. If the returned value is an object, we should return the returned
            // value, so we need to add an if with a `ref.test` here
            WatInstruction::local_get("$new_instance"),
        ])
    }

    fn translate_arrow_function(&mut self, function: &ArrowFunction) -> Box<WatInstruction> {
        self.translate_function_generic(function.name(), function.parameters(), function.body())
    }

    fn translate_update(&mut self, update: &Update) -> Box<WatInstruction> {
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
            UpdateOp::IncrementPost => WatInstruction::call("$increment_number", vec![]),
            UpdateOp::IncrementPre => WatInstruction::call("$increment_number", vec![]),
            UpdateOp::DecrementPost => WatInstruction::call("$decrement_number", vec![]),
            UpdateOp::DecrementPre => WatInstruction::call("$decrement_number", vec![]),
        };
        let target = self.translate_identifier(identifier);
        let offset = self.add_identifier(identifier);
        let set_variable = WatInstruction::call(
            "$set_variable".to_string(),
            vec![
                WatInstruction::local_get("$scope".to_string()),
                WatInstruction::i32_const(offset),
                WatInstruction::local_get("$var"),
            ],
        );

        WatInstruction::list(vec![
            target,
            instruction,
            WatInstruction::local_set("$var"),
            set_variable,
        ])
    }

    fn translate_assign(&mut self, assign: &Assign) -> Box<WatInstruction> {
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
                        WatInstruction::list(vec![
                            rhs,
                            WatInstruction::local_set("$rhs"),
                            WatInstruction::call(
                                "$set_variable".to_string(),
                                vec![
                                    WatInstruction::local_get("$scope".to_string()),
                                    WatInstruction::i32_const(offset),
                                    WatInstruction::local_get("$rhs"),
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

    fn translate_unary(&mut self, unary: &Unary) -> Box<WatInstruction> {
        println!("unary: {:#?}", unary);

        WatInstruction::empty()
    }

    fn translate_literal(&mut self, lit: &Literal) -> Box<WatInstruction> {
        println!("translate_literal: {lit:#?}");
        match lit {
            Literal::Num(num) => {
                WatInstruction::call("$new_number", vec![WatInstruction::f64_const(*num)])
            }
            Literal::String(s) => {
                let s = self.interner.resolve(*s).unwrap().to_string();
                let (offset, length) = self.insert_data_string(&s);

                WatInstruction::call(
                    "$new_string",
                    vec![
                        WatInstruction::i32_const(offset),
                        WatInstruction::i32_const(length),
                    ],
                )
            }
            Literal::Int(i) => {
                WatInstruction::call("$new_number", vec![WatInstruction::f64_const(*i as f64)])
            }
            Literal::BigInt(_big_int) => todo!(),
            Literal::Bool(b) => WatInstruction::call(
                "$new_boolean",
                vec![WatInstruction::i32_const(if *b { 1 } else { 0 })],
            ),
            Literal::Null => WatInstruction::ref_i31(WatInstruction::i32_const(2)),
            Literal::Undefined => WatInstruction::ref_null("any"),
        }
    }

    fn translate_declaration(&mut self, declaration: &Declaration) -> Box<WatInstruction> {
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
                    WatInstruction::call(
                        "$set_variable".to_string(),
                        vec![
                            WatInstruction::local_get("$scope"),
                            WatInstruction::i32_const(offset),
                            declaration,
                        ],
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

    fn translate_statement(&mut self, statement: &Statement) -> Box<WatInstruction> {
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
            Statement::Try(_) => todo!(),
            Statement::With(_with) => todo!(),
        }
    }

    fn translate_throw(&mut self, throw: &Throw) -> Box<WatInstruction> {
        let target = self.translate_expression(throw.target());
        WatInstruction::list(vec![target, WatInstruction::throw("$exception")])
    }

    fn translate_if_statement(&mut self, if_statement: &If) -> Box<WatInstruction> {
        WatInstruction::list(vec![
            self.translate_expression(if_statement.cond()),
            WatInstruction::call("$cast_ref_to_i32_bool", vec![]),
            WatInstruction::r#if(
                None,
                vec![self.translate_statement(if_statement.body())],
                if_statement
                    .else_node()
                    .map(|e| vec![self.translate_statement(e)]),
            ),
        ])
    }

    fn translate_while_loop(&mut self, while_loop: &WhileLoop) -> Box<WatInstruction> {
        println!("condition: {:#?}", while_loop.condition());
        let condition = self.translate_expression(while_loop.condition());
        println!("while_loop: {while_loop:#?}");
        println!("condition: {condition:#?}");
        WatInstruction::r#loop(
            "$while_loop".to_string(),
            vec![WatInstruction::block(
                "$break".into(),
                vec![
                    condition,
                    WatInstruction::call("$cast_ref_to_i32_bool", vec![]),
                    WatInstruction::i32_eqz(),
                    WatInstruction::br_if("$break"),
                    self.translate_statement(while_loop.body()),
                    WatInstruction::br("$while_loop"),
                ],
            )],
        )
    }

    fn translate_block(&mut self, block: &Block) -> Box<WatInstruction> {
        WatInstruction::list(
            block
                .statement_list()
                .statements()
                .iter()
                .map(|s| self.translate_statement_list_item(s))
                .collect(),
        )
    }

    fn translate_statement_list_item(
        &mut self,
        statement: &StatementListItem,
    ) -> Box<WatInstruction> {
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
    println!("{ast:#?}");
    ast.visit_with(&mut translator);
    // exit $init function
    translator.exit_function();

    let init = translator.module.get_function_mut("init").unwrap();
    init.locals
        .insert(("$scope".to_string(), "(ref $Scope)".to_string()));
    init.body
        .push_front(WatInstruction::local_set("$scope".to_string()));
    init.body.push_front(WatInstruction::call(
        "$new_scope",
        vec![WatInstruction::ref_null("$Scope")],
    ));

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
