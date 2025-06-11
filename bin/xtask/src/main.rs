use anyhow::{Result, anyhow};
use grammers_tl_gen::{Config, generate_rust_code};
use grammers_tl_parser::parse_tl_file;
use grammers_tl_parser::tl::Definition;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

/// Find the `// LAYER #` comment, and return its value if it's valid.
fn find_layer(file: &str) -> io::Result<Option<i32>> {
    const LAYER_MARK: &str = "LAYER";

    Ok(BufReader::new(File::open(file)?).lines().find_map(|line| {
        let line = line.unwrap();
        if line.trim().starts_with("//") {
            if let Some(pos) = line.find(LAYER_MARK) {
                if let Ok(layer) = line[pos + LAYER_MARK.len()..].trim().parse() {
                    return Some(layer);
                }
            }
        }

        None
    }))
}

fn load_tl(file: &str) -> io::Result<Vec<Definition>> {
    let mut file = File::open(file)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(parse_tl_file(&contents)
        .filter_map(|d| match d {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("TL: parse error: {:?}", e);
                None
            }
        })
        .collect())
}

fn generate_tl() -> Result<()> {
    let tl_dir = Path::new("../../lib/grammers-tl-types/tl");
    let Some(layer) = find_layer(tl_dir.join("api.tl").to_str().unwrap())? else {
        return Err(anyhow!("layer value not found in api.tl"));
    };
    eprintln!("generating tl LAYER={}", layer);
    let mut defs = vec![];
    for tl in ["api.tl", "mtproto.tl"].map(|v| tl_dir.join(v)) {
        let xs = load_tl(tl.to_str().unwrap())?;
        defs.extend(xs);
    }

    let config = Config {
        gen_name_for_id: true,
        deserializable_functions: true,
        impl_debug: true,
        impl_from_enum: true,
        impl_from_type: true,
    };
    generate_rust_code("__", &defs, layer, &config)?;

    Ok(())
}

fn main() -> Result<()> {
    let task = env::args().nth(1);
    match task.as_deref() {
        Some("generate") => generate_tl()?,
        Some(cmd) => {
            eprintln!("Unknown command: {}", cmd);
        }
        None => {
            eprintln!("Usage: cargo xtask <generate|...>");
        }
    }
    Ok(())
}
