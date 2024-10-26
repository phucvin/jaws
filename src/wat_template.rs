use std::collections::HashMap;

use serde::Serialize;

use tinytemplate::{TinyTemplate, format_unescaped};
use std::error::Error;

#[derive(Serialize)]
struct Context {
    init_code: String,
    data: String,
    free_memory_offset: i32
}

pub fn generate_wat_template(_functions: &HashMap<String, String>, init_code: &str, data: &str, free_memory_offset: i32) -> String {
    let template = std::include_str!("wat/template.wat");
    let mut tt = TinyTemplate::new();
    tt.add_template("module", template).unwrap();
    tt.set_default_formatter(&format_unescaped);
 let context = Context {
        init_code: init_code.to_string(),
        data: data.to_string(),
        free_memory_offset
    };
    tt.render("module", &context).unwrap()
}
