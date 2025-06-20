// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Code to generate Rust's `enum`'s from TL definitions.

use crate::grouper;
use crate::metadata::Metadata;
use crate::rustifier;
use crate::{ignore_type, Config};
use grammers_tl_parser::tl::{Definition, ParameterType, Type};
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// Types that implement Copy from builtin_type
const COPY_TYPES: [&str; 7] = ["bool", "f64", "i32", "i64", "u32", "[u8; 16]", "[u8; 32]"];

/// Writes an enumeration listing all types such as the following rust code:
///
/// ```ignore
/// pub enum Name {
///     Variant(crate::types::Name),
/// }
/// ```
fn write_enum<W: Write>(
    file: &mut W,
    indent: &str,
    ty: &Type,
    metadata: &Metadata,
    config: &Config,
) -> io::Result<()> {
    if config.impl_debug {
        writeln!(file, "{}#[derive(Debug)]", indent)?;
    }

    writeln!(file, "{}#[derive(Clone, PartialEq)]", indent)?;
    writeln!(
        file,
        "{}pub enum {} {{",
        indent,
        rustifier::types::type_name(ty)
    )?;
    for d in metadata.defs_with_type(ty) {
        write!(
            file,
            "{}    {}",
            indent,
            rustifier::definitions::variant_name(d)
        )?;

        // Variant with no struct since it has no data and it only adds noise
        if d.params.is_empty() {
            writeln!(file, ",")?;
            continue;
        } else {
            write!(file, "(")?;
        }

        if metadata.is_recursive_def(d) {
            write!(file, "Box<")?;
        }
        write!(file, "{}", rustifier::definitions::qual_name(d))?;
        if metadata.is_recursive_def(d) {
            write!(file, ">")?;
        }

        writeln!(file, "),")?;
    }
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Writes impl for getting common fields from enum variants
///
/// ```ignore
/// enum Enum {
///     A { id: i64, other: i64 },
///     B { id: i64 }
/// }
///
/// impl Enum {
///     pub fn id(&self) -> i64 {
///         self.id
///     }
/// }
/// ```
fn write_common_field_impl<W: Write>(
    file: &mut W,
    indent: &str,
    ty: &Type,
    metadata: &Metadata,
    _config: &Config,
) -> io::Result<()> {
    // Don't generate if only one type
    let definitions = metadata.defs_with_type(ty);
    if definitions.len() <= 1 {
        return Ok(());
    }
    // Get common parameters
    let mut common_params = HashSet::new();
    for (i, d) in definitions.iter().enumerate() {
        // Filter out Options and flags parameters
        let params: HashSet<_> = d
            .params
            .iter()
            .filter(|p| match p.ty {
                ParameterType::Flags => false,
                ParameterType::Normal { .. } => {
                    !rustifier::parameters::qual_name(p).contains("Option<")
                }
            })
            .collect();
        // Faster
        if params.is_empty() {
            return Ok(());
        }
        // Do intersection
        if i == 0 {
            common_params = params;
            continue;
        }
        common_params = common_params.intersection(&params).copied().collect();
    }
    if common_params.is_empty() {
        return Ok(());
    }
    // Write impl
    writeln!(
        file,
        "{}impl {} {{",
        indent,
        rustifier::types::type_name(ty)
    )?;
    for param in common_params {
        let qual_name = rustifier::parameters::qual_name(param);
        writeln!(
            file,
            "{}    pub fn {}(&self) -> {} {{\n{}        match self {{",
            indent,
            rustifier::parameters::attr_name(param),
            qual_name,
            indent,
        )?;
        // Match cases
        for d in definitions {
            writeln!(
                file,
                "{}            Self::{}(i) => i.{}{},",
                indent,
                rustifier::definitions::variant_name(d),
                rustifier::parameters::attr_name(param),
                // Clone non Copy types
                if COPY_TYPES.contains(&qual_name.as_ref()) {
                    ""
                } else {
                    ".clone()"
                }
            )?;
        }
        writeln!(file, "{}        }}\n{}    }}", indent, indent)?;
    }
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Defines the `impl Serializable` corresponding to the type definitions:
///
/// ```ignore
/// impl crate::Serializable for Name {
///     fn serialize(&self, buf: &mut impl Extend<u8>) {
///         use crate::Identifiable;
///         match self {
///             Self::Variant(x) => {
///                 crate::types::Name::CONSTRUCTOR_ID.serialize(buf);
///                 x.serialize(buf)
///             },
///         }
///     }
/// }
/// ```
fn write_serializable<W: Write>(
    file: &mut W,
    indent: &str,
    ty: &Type,
    metadata: &Metadata,
) -> io::Result<()> {
    writeln!(
        file,
        "{}impl crate::Serializable for {} {{",
        indent,
        rustifier::types::type_name(ty)
    )?;
    writeln!(
        file,
        "{}    fn serialize(&self, buf: &mut impl Extend<u8>) {{",
        indent
    )?;

    // writeln!(file, "{}        use crate::Identifiable;", indent)?;
    writeln!(file, "{}        match self {{", indent)?;
    for d in metadata.defs_with_type(ty) {
        writeln!(
            file,
            "{}            Self::{}{} => {{",
            indent,
            rustifier::definitions::variant_name(d),
            if d.params.is_empty() { "" } else { "(x)" },
        )?;
        writeln!(
            file,
            "{}                {}::CONSTRUCTOR_ID.serialize(buf);",
            indent,
            rustifier::definitions::qual_name(d)
        )?;
        if !d.params.is_empty() {
            writeln!(file, "{}                x.serialize(buf)", indent)?;
        }
        writeln!(file, "{}            }},", indent)?;
    }
    writeln!(file, "{}        }}", indent)?;
    writeln!(file, "{}    }}", indent)?;
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Defines the `impl Deserializable` corresponding to the type definitions:
///
/// ```ignore
/// impl crate::Deserializable for Name {
///     fn deserialize(buf: crate::deserialize::Buffer) -> crate::deserialize::Result<Self> {
///         use crate::Identifiable;
///         Ok(match u32::deserialize(buf)? {
///             crate::types::Name::CONSTRUCTOR_ID => Self::Variant(crate::types::Name::deserialize(buf)?),
///             _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, ...)),
///         })
///     }
/// }
/// ```
fn write_deserializable<W: Write>(
    file: &mut W,
    indent: &str,
    ty: &Type,
    metadata: &Metadata,
) -> io::Result<()> {
    writeln!(
        file,
        "{}impl crate::Deserializable for {} {{",
        indent,
        rustifier::types::type_name(ty)
    )?;
    writeln!(
        file,
        "{}    fn deserialize(buf: crate::deserialize::Buffer) -> crate::deserialize::Result<Self> {{",
        indent
    )?;
    //writeln!(file, "{}        use crate::Identifiable;", indent)?;
    writeln!(file, "{}        let id = u32::deserialize(buf)?;", indent)?;
    writeln!(file, "{}        Ok(match id {{", indent)?;
    for d in metadata.defs_with_type(ty) {
        write!(
            file,
            "{}            {}::CONSTRUCTOR_ID => Self::{}",
            indent,
            rustifier::definitions::qual_name(d),
            rustifier::definitions::variant_name(d),
        )?;

        if d.params.is_empty() {
            writeln!(file, ",")?;
            continue;
        } else {
            write!(file, "(")?;
        }

        if metadata.is_recursive_def(d) {
            write!(file, "Box::new(")?;
        }
        write!(
            file,
            "{}::deserialize(buf)?",
            rustifier::definitions::qual_name(d)
        )?;
        if metadata.is_recursive_def(d) {
            write!(file, ")")?;
        }
        writeln!(file, "),")?;
    }
    writeln!(
        file,
        "{}            _ => return Err(\
         crate::deserialize::Error::UnexpectedConstructor {{ id }}),",
        indent
    )?;
    writeln!(file, "{}        }})", indent)?;
    writeln!(file, "{}    }}", indent)?;
    writeln!(file, "{}}}", indent)?;
    Ok(())
}

/// Defines the `impl From` corresponding to the definition:
///
/// ```ignore
/// impl From<Name> for Enum {
/// }
/// ```
fn write_impl_from<W: Write>(
    file: &mut W,
    indent: &str,
    ty: &Type,
    metadata: &Metadata,
) -> io::Result<()> {
    for def in metadata.defs_with_type(ty) {
        writeln!(
            file,
            "{}impl From<{}> for {} {{",
            indent,
            rustifier::definitions::qual_name(def),
            rustifier::types::type_name(ty),
        )?;
        writeln!(
            file,
            "{}    fn from({}x: {}) -> Self {{",
            indent,
            if def.params.is_empty() { "_" } else { "" },
            rustifier::definitions::qual_name(def),
        )?;
        write!(
            file,
            "{}        {}::{}",
            indent,
            rustifier::types::type_name(ty),
            rustifier::definitions::variant_name(def),
        )?;

        if def.params.is_empty() {
            writeln!(file)?;
        } else if metadata.is_recursive_def(def) {
            writeln!(file, "(Box::new(x))")?;
        } else {
            writeln!(file, "(x)")?;
        }

        writeln!(file, "{}    }}", indent)?;
        writeln!(file, "{}}}", indent)?;
    }
    Ok(())
}

/// Writes an entire definition as Rust code (`enum` and `impl`).
fn write_definition<W: Write>(
    file: &mut W,
    indent: &str,
    ty: &Type,
    metadata: &Metadata,
    config: &Config,
) -> io::Result<()> {
    write_enum(file, indent, ty, metadata, config)?;
    write_common_field_impl(file, indent, ty, metadata, config)?;
    write_serializable(file, indent, ty, metadata)?;
    write_deserializable(file, indent, ty, metadata)?;
    if config.impl_from_type {
        write_impl_from(file, indent, ty, metadata)?;
    }
    Ok(())
}

/// Write the entire module dedicated to enums.
pub(crate) fn write_enums_mod(
    dst_dir: impl AsRef<Path>,
    definitions: &[Definition],
    metadata: &Metadata,
    config: &Config,
) -> io::Result<()> {
    let _ = std::fs::create_dir(dst_dir.as_ref());

    let mut mod_file = File::create(dst_dir.as_ref().join("mod.rs"))?;
    // Begin outermost mod
    write!(
        mod_file,
        "\
         //! This module contains all of the boxed types, each\n\
         //! represented by a `enum`. All of them implement\n\
         //! [`Serializable`] and [`Deserializable`].\n\
         //!\n\
         //! [`Serializable`]: /grammers_tl_types/trait.Serializable.html\n\
         //! [`Deserializable`]: /grammers_tl_types/trait.Deserializable.html\n\
         use crate::Identifiable;\n\
         "
    )?;

    let grouped = grouper::group_types_by_ns(definitions);
    let mut sorted_keys: Vec<&Option<String>> = grouped.keys().collect();
    sorted_keys.sort();
    for key in sorted_keys.into_iter() {
        let file = if let Some(key) = key {
            write!(mod_file, "pub mod {};", key)?;
            &mut File::create(dst_dir.as_ref().join(format!("{}.rs", key)))?
        } else {
            &mut mod_file
        };

        // Begin possibly inner mod
        if key.is_some() {
            writeln!(file, "#![allow(clippy::large_enum_variant)]")?;
            writeln!(file, "use crate::Identifiable;")?;
        }

        for ty in grouped[key].iter().filter(|ty| !ignore_type(ty)) {
            write_definition(file, "", ty, metadata, config)?;
        }
    }

    Ok(())
}
