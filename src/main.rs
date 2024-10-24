use boa_ast::{
    declaration::{Declaration, LexicalDeclaration, VarDeclaration, VariableList},
    expression::{
        literal::Literal,
        operator::{
            binary::{ArithmeticOp, BinaryOp},
            Binary,
        },
        Call, Expression, Identifier,
    },
    function::Function,
    statement::{Return, Statement},
    visitor::{VisitWith, Visitor},
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
        r
    }
}

struct WasmTranslator {
    wat_module: WatModule,
    function_stack: Vec<WatFunction>,
    interner: Interner,
    functions: HashMap<String, String>,
    init_code: Vec<String>,
}

impl WasmTranslator {
    fn new(interner: Interner) -> Self {
        let wat_module = WatModule::new();
        let function = WatFunction::new("init".to_string());
        Self {
            wat_module,
            function_stack: vec![function],
            interner,
            functions: HashMap::new(),
            init_code: Vec::new(),
        }
    }

    fn current_function(&mut self) -> &mut WatFunction {
        self.function_stack.last_mut().unwrap()
    }

    fn enter_function(&mut self, function: WatFunction) {
        self.function_stack.push(function);
    }

    fn exit_function(&mut self) {
        let function = self.function_stack.pop().unwrap();
        self.wat_module.add_function(function);
    }

    fn translate_return(&mut self, ret: &Return) -> Box<WatInstruction> {
        // println!("Return: {ret:#?}");
        let mut instructions = Vec::new();
        if let Some(target) = ret.target() {
            instructions.push(self.translate_expression(target));
        } else {
            instructions.push(WatInstruction::ref_null("any"));
        }
        instructions.push(Box::new(WatInstruction::Return));
        WatInstruction::list(instructions)
    }

    fn translate_function(&mut self, fun: &Function) -> Box<WatInstruction> {
        // println!(
        //     "translate function: {}",
        //     fun.to_interned_string(&self.interner)
        // );
        let function_name =
            gen_function_name(fun.name().map(|i| i.to_interned_string(&self.interner)));
        let mut wat_function = WatFunction::new(function_name.clone());
        self.enter_function(wat_function);

        self.current_function()
            .add_param("$parentScope".to_string(), "(ref $Scope)".to_string());
        self.current_function()
            .add_param("$arguments".to_string(), "(ref null $JSArgs)".to_string());
        self.current_function().add_result("anyref".to_string());

        self.current_function()
            .locals
            .insert(("$scope".to_string(), "(ref $Scope)".to_string()));
        self.current_function()
            .add_instruction(*WatInstruction::call(
                "$new_scope",
                vec![WatInstruction::local_get("$parentScope")],
            ));
        self.current_function()
            .add_instruction(*WatInstruction::local_set("$scope"));

        // set parameters on the scope
        for (i, param) in fun.parameters().as_ref().iter().enumerate() {
            match param.variable().binding() {
                boa_ast::declaration::Binding::Identifier(identifier) => {
                    self.current_function()
                        .add_instruction(WatInstruction::Call {
                            name: "$set_variable".to_string(),
                            args: vec![
                                Box::new(WatInstruction::LocalGet {
                                    name: "$scope".to_string(),
                                }),
                                Box::new(WatInstruction::I32Const {
                                    value: identifier.sym().get() as i32,
                                }),
                                Box::new(WatInstruction::Instruction {
                                    name: "array.get".to_string(),
                                    args: vec![
                                        Box::new(WatInstruction::Type {
                                            name: "JSArgs".to_string(),
                                        }),
                                        Box::new(WatInstruction::LocalGet {
                                            name: "$arguments".to_string(),
                                        }),
                                        Box::new(WatInstruction::I32Const { value: i as i32 }),
                                    ],
                                }),
                            ],
                        });
                }
                boa_ast::declaration::Binding::Pattern(_pattern) => todo!(),
            }
        }

        for statement in fun.body().statements().statements() {
            match statement {
                boa_ast::StatementListItem::Statement(statement) => {
                    // println!(
                    //     "statement: {}",
                    //     statement.to_interned_string(&self.interner)
                    // );
                    let instruction = match statement {
                        Statement::Block(_block) => todo!(),
                        Statement::Var(_var_declaration) => todo!(),
                        Statement::Empty => Box::new(WatInstruction::Empty), //Box::new(WatInstruction::Block { instructions: Vec::new() }),
                        Statement::Expression(expression) => self.translate_expression(expression),
                        Statement::If(_) => todo!(),
                        Statement::DoWhileLoop(_do_while_loop) => todo!(),
                        Statement::WhileLoop(_while_loop) => todo!(),
                        Statement::ForLoop(_for_loop) => todo!(),
                        Statement::ForInLoop(_for_in_loop) => todo!(),
                        Statement::ForOfLoop(_for_of_loop) => todo!(),
                        Statement::Switch(_switch) => todo!(),
                        Statement::Continue(_) => todo!(),
                        Statement::Break(_) => todo!(),
                        Statement::Return(ret) => self.translate_return(ret),
                        Statement::Labelled(_labelled) => todo!(),
                        Statement::Throw(_throw) => todo!(),
                        Statement::Try(_) => todo!(),
                        Statement::With(_with) => todo!(),
                    };
                    self.current_function().add_instruction(*instruction);
                }
                boa_ast::StatementListItem::Declaration(declaration) => {
                    let declaration = self.translate_declaration(declaration);
                    self.current_function().add_instruction(*declaration);
                }
            }
        }

        // let function_body = self.current_function()
        //     .body
        //     .iter()
        //     .map(|instr| instr.to_string())
        //     .collect::<Vec<_>>()
        //     .join("\n");
        // self.functions.insert(function_name.clone(), function_body);
        self.exit_function();

        Box::new(WatInstruction::Call {
            name: "$new_function".to_string(),
            args: vec![
                Box::new(WatInstruction::LocalGet {
                    name: "$scope".to_string(),
                }),
                Box::new(WatInstruction::RefFunc {
                    name: function_name,
                }),
            ],
        })
    }

    fn translate_lexical(&mut self, decl: &LexicalDeclaration) -> Box<WatInstruction> {
        // println!(
        //     "translate lexical {}",
        //     decl.to_interned_string(&self.interner)
        // );
        match decl {
            LexicalDeclaration::Const(variable_list) => todo!(),
            LexicalDeclaration::Let(variable_list) => self.translate_let_vars(variable_list),
        }
    }

    fn translate_let(&mut self, decl: &VarDeclaration) -> Box<WatInstruction> {
        // println!("LET: {}", decl.to_interned_string(&self.interner));
        // Implementation for let declaration
        todo!()
    }

    fn translate_call(&mut self, call: &Call) -> Box<WatInstruction> {
        // println!(
        //     "translate_call {}",
        //     call.function().to_interned_string(&self.interner)
        // );
        let function_name = call.function().to_interned_string(&self.interner);
        if function_name == "log" {
            // Specialcasing log for now
            let arg = &call.args()[0];
            if let Expression::Identifier(identifier) = arg {
                let mut instructions = Vec::new();
                let call_instr = WatInstruction::call(
                    "$get_variable",
                    vec![
                        WatInstruction::local_get("$scope"),
                        WatInstruction::i32_const(identifier.sym().get() as i32),
                    ],
                );
                let cast = WatInstruction::instruction(
                    "ref.cast",
                    vec![
                        Box::new(WatInstruction::Ref("Number".to_string())),
                        call_instr,
                    ],
                );
                instructions.push(WatInstruction::instruction(
                    "struct.get",
                    vec![
                        WatInstruction::type_("Number"),
                        Box::new(WatInstruction::Identifier("0".to_string())),
                        cast,
                    ],
                ));
                instructions.push(WatInstruction::call("$log", vec![]));

                WatInstruction::list(instructions)
            } else {
                panic!("unreachable")
            }
            // (struct.get $Number 0 (ref.cast (ref $Number) (call $get_variable (local.get $scope) (i32.const 73))))
            // (call $log)
        } else {
            let mut instructions = Vec::new();

            // Add a local for arguments to the current function
            self.current_function()
                .add_local("$call_arguments".to_string(), "(ref $JSArgs)".to_string());

            // Create the arguments array
            let args_count = call.args().len() as i32;
            instructions.push(Box::new(WatInstruction::ArrayNew {
                name: "$JSArgs".to_string(),
                init: Box::new(WatInstruction::RefNull {
                    type_: "any".to_string(),
                }),
                length: WatInstruction::i32_const(args_count),
            }));
            instructions.push(Box::new(WatInstruction::LocalSet {
                name: "$call_arguments".to_string(),
            }));

            // instructions.push(Box::new(WatInstruction::I32Const {
            //     value: args_count,
            // }));
            // Populate the arguments array
            for (index, arg) in call.args().iter().enumerate() {
                let arg_instruction = self.translate_expression(arg);
                instructions.push(Box::new(WatInstruction::Instruction {
                    name: "array.set".to_string(),
                    args: vec![
                        Box::new(WatInstruction::Type {
                            name: "JSArgs".to_string(),
                        }),
                        Box::new(WatInstruction::LocalGet {
                            name: "$call_arguments".to_string(),
                        }),
                        Box::new(WatInstruction::I32Const {
                            value: index as i32,
                        }),
                        arg_instruction,
                    ],
                }));
            }

            // Translate the function expression
            self.current_function()
                .locals
                .insert(("$function".to_string(), "anyref".to_string()));
            instructions.push(self.translate_expression(call.function()));
            instructions.push(WatInstruction::local_set("$function"));

            // Call the function
            instructions.push(Box::new(WatInstruction::Call {
                name: "$call_function".to_string(),
                args: vec![
                    Box::new(WatInstruction::LocalGet {
                        name: "$scope".to_string(),
                    }),
                    Box::new(WatInstruction::LocalGet {
                        name: "$function".to_string(),
                    }),
                    Box::new(WatInstruction::LocalGet {
                        name: "$call_arguments".to_string(),
                    }),
                ],
            }));

            Box::new(WatInstruction::List { instructions })
        }
    }

    fn translate_let_vars(&mut self, variable_list: &VariableList) -> Box<WatInstruction> {
        // TODO: refactor getting current function

        self.current_function()
            .locals
            .insert(("$var".to_string(), "anyref".to_string()));

        let mut instructions = Vec::new();
        for var in variable_list.as_ref() {
            let var_name = var.binding().to_interned_string(&self.interner);
            if let Some(expression) = var.init() {
                match var.binding() {
                    boa_ast::declaration::Binding::Identifier(identifier) => {
                        // println!("var_name = {var_name} (sym: {})", identifier.sym().get());

                        instructions.push(self.translate_expression(expression));
                        instructions.push(WatInstruction::local_set("$var"));

                        instructions.push(Box::new(WatInstruction::LocalGet {
                            name: "$scope".to_string(),
                        }));
                        instructions.push(Box::new(WatInstruction::I32Const {
                            value: identifier.sym().get() as i32,
                        }));
                        instructions.push(WatInstruction::local_get("$var"));
                        instructions.push(Box::new(WatInstruction::Call {
                            name: "$set_variable".to_string(),
                            args: vec![],
                        }));
                    }
                    boa_ast::declaration::Binding::Pattern(pattern) => todo!(),
                }
            }
        }

        Box::new(WatInstruction::List { instructions })
    }

    fn translate_binary(&mut self, binary: &Binary) -> Box<WatInstruction> {
        // println!("Binary: {binary:#?}");
        match binary.op() {
            BinaryOp::Arithmetic(ArithmeticOp::Add) => {
                let lhs = self.translate_expression(binary.lhs());
                let rhs = self.translate_expression(binary.rhs());
                Box::new(WatInstruction::Call {
                    name: "$add".to_string(),
                    args: vec![lhs, rhs],
                })
            }
            _ => {
                // println!("Unsupported binary operation: {:?}", binary.op());
                Box::new(WatInstruction::List {
                    instructions: Vec::new(),
                })
            }
        }
    }

    fn translate_identifier(&mut self, identifier: &Identifier) -> Box<WatInstruction> {
        Box::new(WatInstruction::Call {
            name: "$get_variable".to_string(),
            args: vec![
                Box::new(WatInstruction::LocalGet {
                    name: "$scope".to_string(),
                }),
                Box::new(WatInstruction::I32Const {
                    value: identifier.sym().get() as i32,
                }),
            ],
        })
    }

    fn translate_expression(&mut self, expression: &Expression) -> Box<WatInstruction> {
        // println!(
        //     "translate expression {}",
        //     expression.to_interned_string(&self.interner)
        // );
        match expression {
            Expression::This => todo!(),
            Expression::Identifier(identifier) => self.translate_identifier(identifier),
            Expression::Literal(literal) => self.translate_literal(literal),
            Expression::RegExpLiteral(_reg_exp_literal) => todo!(),
            Expression::ArrayLiteral(_array_literal) => todo!(),
            Expression::ObjectLiteral(_object_literal) => todo!(),
            Expression::Spread(_spread) => todo!(),
            Expression::Function(function) => self.translate_function(function),
            Expression::ArrowFunction(_arrow_function) => todo!(),
            Expression::AsyncArrowFunction(_async_arrow_function) => todo!(),
            Expression::Generator(_generator) => todo!(),
            Expression::AsyncFunction(_async_function) => todo!(),
            Expression::AsyncGenerator(_async_generator) => todo!(),
            Expression::Class(_class) => todo!(),
            Expression::TemplateLiteral(_template_literal) => todo!(),
            Expression::PropertyAccess(_property_access) => {
                // println!("property_access: {:#?}", _property_access);
                todo!()
            }
            Expression::New(_) => todo!(),
            Expression::Call(call) => self.translate_call(call),
            Expression::SuperCall(_super_call) => todo!(),
            Expression::ImportCall(_import_call) => todo!(),
            Expression::Optional(_optional) => todo!(),
            Expression::TaggedTemplate(_tagged_template) => todo!(),
            Expression::NewTarget => todo!(),
            Expression::ImportMeta => todo!(),
            Expression::Assign(_assign) => todo!(),
            Expression::Unary(_unary) => todo!(),
            Expression::Update(_update) => todo!(),
            Expression::Binary(binary) => self.translate_binary(binary),
            Expression::BinaryInPrivate(_binary_in_private) => todo!(),
            Expression::Conditional(_conditional) => todo!(),
            Expression::Await(_) => todo!(),
            Expression::Yield(_) => todo!(),
            Expression::Parenthesized(parenthesized) => todo!(),
            _ => todo!(),
        }
    }

    fn translate_literal(&mut self, lit: &Literal) -> Box<WatInstruction> {
        match lit {
            Literal::Num(num) => {
                WatInstruction::call("$new_number", vec![WatInstruction::f64_const(*num)])
            }
            Literal::String(s) => WatInstruction::call(
                "$new_string",
                vec![WatInstruction::i32_const(self.add_string_constant(s) as i32)],
            ),
            Literal::Int(i) => {
                WatInstruction::call("$new_number", vec![WatInstruction::f64_const(*i as f64)])
            }
            Literal::BigInt(_big_int) => todo!(),
            Literal::Bool(b) => WatInstruction::call(
                "$new_boolean",
                vec![WatInstruction::i32_const(if *b { 1 } else { 0 })],
            ),
            Literal::Null => WatInstruction::ref_null("any"),
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
                    Box::new(WatInstruction::Call {
                        name: "$set_variable".to_string(),
                        args: vec![
                            Box::new(WatInstruction::LocalGet {
                                name: "$scope".to_string(),
                            }),
                            Box::new(WatInstruction::I32Const {
                                value: name.sym().get() as i32,
                            }),
                            declaration,
                        ],
                    })
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

    fn add_string_constant(&mut self, s: &Sym) -> i32 {
        // Implement string constant management here
        0
    }
}

impl<'a> Visitor<'a> for WasmTranslator {
    type BreakTy = ();

    fn visit_var_declaration(&mut self, node: &'a VarDeclaration) -> ControlFlow<Self::BreakTy> {
        // println!(
        //     "visit_var_declaration: {}",
        //     node.to_interned_string(&self.interner)
        // );
        let instruction = self.translate_let(node);
        let current_function = self.current_function().clone();
        self.current_function().add_instruction(*instruction);
        ControlFlow::Continue(())
    }

    fn visit_declaration(&mut self, node: &Declaration) -> ControlFlow<Self::BreakTy> {
        // println!(
        //     "visit_declaration: {}",
        //     node.to_interned_string(&self.interner)
        // );
        let instruction = self.translate_declaration(node);
        let current_function = self.current_function().clone();
        self.current_function().add_instruction(*instruction);
        ControlFlow::Continue(())
    }

    fn visit_expression(&mut self, node: &Expression) -> ControlFlow<Self::BreakTy> {
        // println!(
        //     "visit_expression: {}",
        //     node.to_interned_string(&self.interner)
        // );
        let instruction = self.translate_expression(node);
        let current_function = self.current_function().clone();
        self.current_function().add_instruction(*instruction);
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
    translator.exit_function();

    let init = translator.wat_module.get_function_mut("init").unwrap();
    init.locals
        .insert(("$scope".to_string(), "(ref $Scope)".to_string()));
    init.body.push_front(Box::new(WatInstruction::LocalSet {
        name: "$scope".to_string(),
    }));
    init.body.push_front(Box::new(WatInstruction::Call {
        name: "$new_scope".to_string(),
        args: vec![Box::new(WatInstruction::RefNull {
            type_: "$Scope".to_string(),
        })],
    }));

    // Generate the full WAT module
    let wat_module = translator.wat_module.to_string();

    // Generate the full WAT module using the template
    let wat_module_with_template =
        wat_template::generate_wat_template(&translator.functions, &wat_module);

    let mut f = File::create("wat/generated.wat").unwrap();
    f.write_all(wat_module_with_template.as_bytes()).unwrap();

    println!("WAT modules generated successfully!");
    Ok(())
}
