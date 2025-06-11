// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! This module gathers all the code generation submodules and coordinates
//! them, feeding them the right data.
mod enums;
mod grouper;
mod metadata;
mod rustifier;
mod structs;

use grammers_tl_parser::tl::{Category, Definition, Type};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

pub struct Config {
    pub gen_name_for_id: bool,
    pub deserializable_functions: bool,
    pub impl_debug: bool,
    pub impl_from_type: bool,
    pub impl_from_enum: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gen_name_for_id: false,
            deserializable_functions: false,
            impl_debug: true,
            impl_from_type: true,
            impl_from_enum: true,
        }
    }
}

/// Don't generate types for definitions of this type,
/// since they are "core" types and treated differently.
const SPECIAL_CASED_TYPES: [&str; 1] = ["Bool"];

fn ignore_type(ty: &Type) -> bool {
    SPECIAL_CASED_TYPES.iter().any(|&x| x == ty.name)
}

pub fn generate_rust_code(
    dst_dir: impl AsRef<Path>,
    definitions: &[Definition],
    layer: i32,
    config: &Config,
) -> io::Result<()> {
    let _ = std::fs::create_dir(dst_dir.as_ref());

    let mut mod_file = File::create(dst_dir.as_ref().join("mod.rs"))?;
    writeln!(
        mod_file,
        r#"
// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

/// The schema layer from which the definitions were generated.
pub const LAYER: i32 = {};
"#,
        layer
    )?;

    if config.gen_name_for_id {
        writeln!(
            mod_file,
            r#"
/// Return the name from the `.tl` definition corresponding to the provided definition identifier.
pub fn name_for_id(id: u32) -> &'static str {{
    match id {{
        0x1cb5c415 => "vector","#
        )?;
        for def in definitions {
            writeln!(
                mod_file,
                r#"        0x{:x} => "{}","#,
                def.id,
                def.full_name()
            )?;
        }

        writeln!(
            mod_file,
            r#"
        _ => "(unknown)",
    }}
}}
    "#,
        )?;
    }

    let metadata = metadata::Metadata::new(definitions);
    structs::write_category_mod(
        dst_dir.as_ref().join("types"),
        Category::Types,
        definitions,
        &metadata,
        config,
    )?;
    writeln!(mod_file, "pub mod types;")?;

    structs::write_category_mod(
        dst_dir.as_ref().join("functions"),
        Category::Functions,
        definitions,
        &metadata,
        config,
    )?;
    writeln!(mod_file, "pub mod functions;")?;

    enums::write_enums_mod(
        dst_dir.as_ref().join("enums"),
        definitions,
        &metadata,
        config,
    )?;
    writeln!(mod_file, "pub mod enums;")?;

    Ok(())
}
