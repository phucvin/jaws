use std::collections::HashMap;

use serde::Serialize;

use std::error::Error;
use tinytemplate::{format_unescaped, TinyTemplate};

#[derive(Serialize)]
struct Context {
    init_code: String,
    data: String,
    free_memory_offset: i32,
    additional_functions: String,
}

pub fn generate_wat_template(
    additional_functions: impl Into<String>,
    init_code: impl Into<String>,
    data: impl Into<String>,
    free_memory_offset: i32,
) -> String {
    let template = std::include_str!("wat/template.wat");
    let mut tt = TinyTemplate::new();
    tt.add_template("module", template).unwrap();
    tt.set_default_formatter(&format_unescaped);
    let context = Context {
        init_code: init_code.into(),
        data: data.into(),
        free_memory_offset,
        additional_functions: additional_functions.into(),
    };
    tt.render("module", &context).unwrap()
}
