// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Code to generate Rust's `struct`'s from TL definitions.

use crate::grouper;
use crate::metadata::Metadata;
use crate::rustifier;
use crate::{Config, ignore_type};
use grammers_tl_parser::tl::{Category, Definition, ParameterType};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// Get the list of generic parameters:
///
/// ```ignore
/// <X, Y>
/// ```
fn get_generic_param_list(def: &Definition, trait_bounds: &str) -> String {
    let mut result = String::new();
    for param in def.params.iter() {
        match param.ty {
            ParameterType::Flags => {}
            ParameterType::Normal { ref ty, .. } => {
                if ty.generic_ref {
                    if result.is_empty() {
                        result.push('<');
                    } else {
                        result.push_str(", ");
                    }
                    result.push_str(&ty.name);
                    result.push_str(trait_bounds);
                }
            }
        }
    }
    if !result.is_empty() {
        result.push('>');
    }
    result
}

/// Defines the `struct` corresponding to the definition:
///
/// ```ignore
/// pub struct Name {
///     pub field: Type,
/// }
/// ```
fn write_struct<W: Write>(
    file: &mut W,
    indent: &str,
    def: &Definition,
    _metadata: &Metadata,
    config: &Config,
) -> io::Result<()> {
    // Define struct
    if config.impl_debug {
        writeln!(file, "{}#[derive(Debug)]", indent)?;
    }

    writeln!(file, "{}#[derive(Clone, PartialEq)]", indent)?;
    write!(
        file,
        "{}pub struct {}{} {{",
        indent,
        rustifier::definitions::type_name(def),
        get_generic_param_list(def, ""),
    )?;

    writeln!(file)?;
    for param in def.params.iter() {
        match param.ty {
            ParameterType::Flags => {
                // Flags are computed on-the-fly, not stored
            }
            ParameterType::Normal { .. } => {
                writeln!(
                    file,
                    "{}    pub {}: {},",
                    indent,
                    rustifier::parameters::attr_name(param),
                    rustifier::parameters::qual_name(param),
                )?;
            }
        }
    }
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Defines the `impl Identifiable` corresponding to the definition:
///
/// ```ignore
/// impl crate::Identifiable for Name {
///     fn constructor_id() -> u32 { 123 }
/// }
/// ```
fn write_identifiable<W: Write>(
    file: &mut W,
    indent: &str,
    def: &Definition,
    _metadata: &Metadata,
) -> io::Result<()> {
    writeln!(
        file,
        "{}impl{} crate::Identifiable for {}{} {{",
        indent,
        get_generic_param_list(def, ""),
        rustifier::definitions::type_name(def),
        get_generic_param_list(def, ""),
    )?;
    writeln!(
        file,
        "{}    const CONSTRUCTOR_ID: u32 = {};",
        indent, def.id
    )?;
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Defines the `impl Serializable` corresponding to the definition:
///
/// ```ignore
/// impl crate::Serializable for Name {
///     fn serialize(&self, buf: &mut impl Extend<u8>) {
///         self.field.serialize(buf);
///     }
/// }
/// ```
fn write_serializable<W: Write>(
    file: &mut W,
    indent: &str,
    def: &Definition,
    _metadata: &Metadata,
) -> io::Result<()> {
    writeln!(
        file,
        "{}impl{} crate::Serializable for {}{} {{",
        indent,
        get_generic_param_list(def, ": crate::Serializable"),
        rustifier::definitions::type_name(def),
        get_generic_param_list(def, ""),
    )?;
    writeln!(
        file,
        "{}    fn serialize(&self, {}buf: &mut impl Extend<u8>) {{",
        indent,
        if def.category == Category::Types && def.params.is_empty() {
            "_"
        } else {
            ""
        }
    )?;

    match def.category {
        Category::Types => {
            // Bare types should not write their `CONSTRUCTOR_ID`.
        }
        Category::Functions => {
            // Functions should always write their `CONSTRUCTOR_ID`.
            writeln!(file, "{}        use crate::Identifiable;", indent)?;
            writeln!(
                file,
                "{}        Self::CONSTRUCTOR_ID.serialize(buf);",
                indent
            )?;
        }
    }

    for param in def.params.iter() {
        write!(file, "{}        ", indent)?;
        match &param.ty {
            ParameterType::Flags => {
                write!(file, "(0u32")?;

                // Compute flags as a single expression
                for p in def.params.iter() {
                    match &p.ty {
                        ParameterType::Normal {
                            ty,
                            flag: Some(flag),
                        } if flag.name == param.name => {
                            // We make sure this `p` uses the flag we're currently
                            // parsing by comparing (`p`'s) `flag.name == param.name`.

                            // OR (if the flag is present) the correct bit index.
                            // Only the special-cased "true" flags are booleans.
                            write!(
                                file,
                                " | if self.{}{} {{ {} }} else {{ 0 }}",
                                rustifier::parameters::attr_name(p),
                                if ty.name == "true" { "" } else { ".is_some()" },
                                1 << flag.index
                            )?;
                        }
                        _ => {}
                    }
                }

                writeln!(file, ").serialize(buf);")?;
            }
            ParameterType::Normal { ty, flag } => {
                // The `true` bare type is a bit special: it's empty so there
                // is not need to serialize it, but it's used enough to deserve
                // a special case and ignore it.
                if ty.name != "true" {
                    if flag.is_some() {
                        writeln!(
                            file,
                            "if let Some(ref x) = self.{} {{ ",
                            rustifier::parameters::attr_name(param)
                        )?;
                        writeln!(file, "{}            x.serialize(buf);", indent)?;
                        writeln!(file, "{}        }}", indent)?;
                    } else {
                        writeln!(
                            file,
                            "self.{}.serialize(buf);",
                            rustifier::parameters::attr_name(param)
                        )?;
                    }
                }
            }
        }
    }

    writeln!(file, "{}    }}", indent)?;
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Defines the `impl Deserializable` corresponding to the definition:
///
/// ```ignore
/// impl crate::Deserializable for Name {
///     fn deserialize(buf: crate::deserialize::Buffer) -> crate::deserialize::Result<Self> {
///         let field = FieldType::deserialize(buf)?;
///         Ok(Name { field })
///     }
/// }
/// ```
fn write_deserializable<W: Write>(
    file: &mut W,
    indent: &str,
    def: &Definition,
    metadata: &Metadata,
) -> io::Result<()> {
    writeln!(
        file,
        "{}impl{} crate::Deserializable for {}{} {{",
        indent,
        get_generic_param_list(def, ": crate::Deserializable"),
        rustifier::definitions::type_name(def),
        get_generic_param_list(def, ""),
    )?;
    writeln!(
        file,
        "{}    fn deserialize({}buf: crate::deserialize::Buffer) -> crate::deserialize::Result<Self> {{",
        indent,
        if def.params.is_empty() { "_" } else { "" }
    )?;

    for param in def.params.iter() {
        write!(file, "{}        ", indent)?;
        match &param.ty {
            ParameterType::Flags => {
                writeln!(
                    file,
                    "let {}{} = u32::deserialize(buf)?;",
                    if metadata.is_unused_flag(def, param) {
                        "_"
                    } else {
                        ""
                    },
                    rustifier::parameters::attr_name(param)
                )?;
            }
            ParameterType::Normal { ty, flag } => {
                if ty.name == "true" {
                    let flag = flag
                        .as_ref()
                        .expect("the `true` type must always be used in a flag");
                    writeln!(
                        file,
                        "let {} = ({} & {}) != 0;",
                        rustifier::parameters::attr_name(param),
                        flag.name,
                        1 << flag.index
                    )?;
                } else {
                    write!(file, "let {} = ", rustifier::parameters::attr_name(param))?;
                    if let Some(flag) = flag {
                        writeln!(file, "if ({} & {}) != 0 {{", flag.name, 1 << flag.index)?;
                        write!(file, "{}            Some(", indent)?;
                    }
                    if ty.generic_ref {
                        write!(file, "{}::deserialize(buf)?", ty.name)?;
                    } else {
                        write!(
                            file,
                            "{}::deserialize(buf)?",
                            rustifier::types::item_path(ty)
                        )?;
                    }
                    if flag.is_some() {
                        writeln!(file, ")")?;
                        writeln!(file, "{}        }} else {{", indent)?;
                        writeln!(file, "{}            None", indent)?;
                        write!(file, "{}        }}", indent)?;
                    }
                    writeln!(file, ";")?;
                }
            }
        }
    }

    writeln!(
        file,
        "{}        Ok({} {{",
        indent,
        rustifier::definitions::type_name(def)
    )?;

    for param in def.params.iter() {
        write!(file, "{}            ", indent)?;
        match &param.ty {
            ParameterType::Flags => {}
            ParameterType::Normal { .. } => {
                writeln!(file, "{},", rustifier::parameters::attr_name(param))?;
            }
        }
    }
    writeln!(file, "{}        }})", indent)?;
    writeln!(file, "{}    }}", indent)?;
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Defines the `impl RemoteCall` corresponding to the definition:
///
/// ```ignore
/// impl crate::RemoteCall for Name {
///     type Return = Name;
/// }
/// ```
fn write_rpc<W: Write>(
    file: &mut W,
    indent: &str,
    def: &Definition,
    _metadata: &Metadata,
) -> io::Result<()> {
    writeln!(
        file,
        "{}impl{} crate::RemoteCall for {}{} {{",
        indent,
        get_generic_param_list(def, ": crate::RemoteCall"),
        rustifier::definitions::type_name(def),
        get_generic_param_list(def, ""),
    )?;
    writeln!(
        file,
        "{}    type Return = {}{};",
        indent,
        rustifier::types::qual_name(&def.ty),
        if def.ty.generic_ref { "::Return" } else { "" },
    )?;
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Defines the `impl From` or `impl TryFrom` corresponding to the definition:
///
/// ```ignore
/// impl From<Enum> for Name {
/// }
///
/// impl TryFrom<Enum> for Name {
///     type Error = ();
/// }
/// ```
fn write_impl_from<W: Write>(
    file: &mut W,
    indent: &str,
    def: &Definition,
    metadata: &Metadata,
) -> io::Result<()> {
    let infallible = metadata.defs_with_type(&def.ty).len() == 1;
    let type_name = rustifier::definitions::type_name(def);

    writeln!(
        file,
        "{}impl {}From<{}> for {} {{",
        indent,
        if infallible { "" } else { "Try" },
        rustifier::types::qual_name(&def.ty),
        type_name,
    )?;
    if !infallible {
        writeln!(file, "{}    type Error = ();", indent)?;
    }
    writeln!(
        file,
        "{}    fn {try_}from(x: {cls}) -> {result}Self{error} {{",
        indent,
        try_ = if infallible { "" } else { "try_" },
        cls = rustifier::types::qual_name(&def.ty),
        result = if infallible { "" } else { "Result<" },
        error = if infallible { "" } else { ", Self::Error>" },
    )?;
    writeln!(file, "{}        match x {{", indent)?;
    writeln!(
        file,
        "{}            {cls}::{name}{data} => {ok}{deref}{value}{body}{paren},",
        indent,
        cls = rustifier::types::qual_name(&def.ty),
        name = rustifier::definitions::variant_name(def),
        data = if def.params.is_empty() { "" } else { "(x)" },
        ok = if infallible { "" } else { "Ok(" },
        deref = if metadata.is_recursive_def(def) {
            "*"
        } else {
            ""
        },
        value = if def.params.is_empty() {
            type_name.as_ref()
        } else {
            "x"
        },
        body = if def.params.is_empty() { " {}" } else { "" },
        paren = if infallible { "" } else { ")" },
    )?;
    if !infallible {
        writeln!(file, "{}            _ => Err(())", indent)?;
    }
    writeln!(file, "{}        }}", indent)?;
    writeln!(file, "{}    }}", indent)?;
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Writes an entire definition as Rust code (`struct` and `impl`).
fn write_definition<W: Write>(
    file: &mut W,
    indent: &str,
    def: &Definition,
    metadata: &Metadata,
    config: &Config,
) -> io::Result<()> {
    write_struct(file, indent, def, metadata, config)?;
    write_identifiable(file, indent, def, metadata)?;
    write_serializable(file, indent, def, metadata)?;
    if def.category == Category::Types || config.deserializable_functions {
        write_deserializable(file, indent, def, metadata)?;
    }
    if def.category == Category::Functions {
        write_rpc(file, indent, def, metadata)?;
    }
    if def.category == Category::Types && config.impl_from_enum {
        write_impl_from(file, indent, def, metadata)?;
    }
    Ok(())
}

/// Write an entire module for the desired category.
pub(crate) fn write_category_mod(
    dst_dir: impl AsRef<Path>,
    category: Category,
    definitions: &[Definition],
    metadata: &Metadata,
    config: &Config,
) -> io::Result<()> {
    let _ = std::fs::create_dir(dst_dir.as_ref());

    let mut mod_file = File::create(dst_dir.as_ref().join("mod.rs"))?;

    let grouped = grouper::group_by_ns(definitions, category);
    let mut sorted_keys: Vec<&String> = grouped.keys().collect();
    sorted_keys.sort();
    for key in sorted_keys.into_iter() {
        let file = if key.is_empty() {
            &mut mod_file
        } else {
            writeln!(mod_file, "pub mod {};", key)?;
            &mut File::create(dst_dir.as_ref().join(format!("{}.rs", key)))?
        };

        if !key.is_empty() {
            writeln!(file, "#![allow(clippy::unreadable_literal)]")?;
        }

        match category {
            Category::Types => {
                write!(
                    file,
                    "\
                     //! This module contains all of the bare types, each\n\
                     //! represented by a `struct`. All of them implement\n\
                     //! [`Identifiable`], [`Serializable`] and [`Deserializable`].\n\
                     //!\n\
                     //! [`Identifiable`]: ../trait.Identifiable.html\n\
                     //! [`Serializable`]: ../trait.Serializable.html\n\
                     //! [`Deserializable`]: ../trait.Deserializable.html\n\
                     #![allow(clippy::cognitive_complexity, clippy::identity_op, clippy::unreadable_literal)]\n\
                     #![allow(unused_imports)]\n\
                     "
                )?;
            }
            Category::Functions => {
                writeln!(
                    file,
                    "\
                //! This module contains all of the functions, each\n\
                //! represented by a `struct`. All of them implement\n\
                //! [`Identifiable`] and [`Serializable`].\n\
                //!\n\
                //! To find out the type that Telegram will return upon\n\
                //! invoking one of these requests, check out the associated\n\
                //! type in the corresponding [`RemoteCall`] trait impl.\n\
                //!\n\
                //! [`Identifiable`]: ../trait.Identifiable.html\n\
                //! [`Serializable`]: ../trait.Serializable.html\n\
                //! [`RemoteCall`]: trait.RemoteCall.html\n\
                #![allow(clippy::cognitive_complexity, clippy::identity_op, clippy::unreadable_literal)]\n\
                #![allow(unused_imports)]\n\
                "
                )?;
            }
        }
        // Begin possibly inner mod
        if category == Category::Types && config.impl_from_enum {
            // If all of the conversions are infallible this will be unused.
            // Don't bother checking this beforehand, just allow warnings.
            writeln!(file, "use std::convert::TryFrom;")?;
        }
        writeln!(file, "use crate::Identifiable;")?;

        for definition in grouped[key]
            .iter()
            .filter(|def| def.category == Category::Functions || !ignore_type(&def.ty))
        {
            write_definition(file, "", definition, metadata, config)?;
        }
    }

    Ok(())
}
