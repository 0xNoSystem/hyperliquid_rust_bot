use std::{env, fs, path::PathBuf};

fn snake_to_pascal(s: &str) -> String {
    s.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let strats_dir = manifest_dir.join("src/strats");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let mut mod_decls = Vec::new();
    let mut use_decls = Vec::new();
    let mut enum_variants = Vec::new();
    let mut indicators_arms = Vec::new();
    let mut init_arms = Vec::new();

    for entry in fs::read_dir(&strats_dir).unwrap() {
        let path = entry.unwrap().path();

        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        if path.file_name().unwrap() == "mod.rs" {
            continue;
        }

        let file = path.file_stem().unwrap().to_str().unwrap();
        let pascal = snake_to_pascal(file);

        // Absolute path to the real source file
        let abs_path = manifest_dir
            .join("src/strats")
            .join(format!("{file}.rs"))
            .display()
            .to_string();

        mod_decls.push(format!("#[path = \"{abs_path}\"] mod {file};"));

        use_decls.push(format!("pub use {file}::{pascal};"));

        enum_variants.push(pascal.clone());

        indicators_arms.push(format!(
            "Strategy::{0} => {0}::required_indicators_static(),",
            pascal
        ));

        init_arms.push(format!("Strategy::{0} => Box::new({0}::init()),", pascal));
    }

    let generated = format!(
        r#"
// AUTO-GENERATED — DO NOT EDIT

{mods}

{uses}

#[derive(Clone, Debug, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {{
    {variants}
}}

impl Strategy {{
    pub fn indicators(&self) -> Vec<IndexId> {{
        match self {{
            {indicators}
        }}
    }}

    pub fn init(&self) -> Box<dyn Strat> {{
        match self {{
            {inits}
        }}
    }}
}}
"#,
        mods = mod_decls.join("\n"),
        uses = use_decls.join("\n"),
        variants = enum_variants.join(",\n    "),
        indicators = indicators_arms.join("\n            "),
        inits = init_arms.join("\n            "),
    );

    let out_file = out_dir.join("strats_gen.rs");
    fs::write(&out_file, generated).unwrap();

    // ensure regeneration when strategies change
    println!("cargo:rerun-if-changed=src/strats");
}
