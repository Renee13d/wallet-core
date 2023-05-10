// Copyright © 2017-2023 Trust Wallet.
//
// This file is part of Trust. The full Trust copyright notice, including
// terms governing use, modification, and redistribution, is contained in the
// file LICENSE at the root of the source code distribution tree.

use super::pretty_object_method_name;
use super::*;
use crate::manifest::{FunctionInfo, TypeVariant};

/// This function checks each function and determines whether there's an
/// association with the passed on object (struct or enum), based on common name
/// prefix, and maps the data into a Swift structure.
///
/// This function returns a tuple of associated Swift functions and the skipped
/// respectively non-associated functions.
pub(super) fn process_methods(
    object: &ObjectVariant,
    functions: Vec<FunctionInfo>,
) -> Result<(Vec<SwiftFunction>, Vec<FunctionInfo>)> {
    let mut swift_funcs = vec![];
    let mut skipped_funcs = vec![];

    for func in functions {
        match do_process_methods(object, &func)? {
            Some(f) => {
                swift_funcs.push(f);
            }
            None => {
                skipped_funcs.push(func);
            }
        }
    }

    Ok((swift_funcs, skipped_funcs))
}

fn do_process_methods(
    object: &ObjectVariant,
    func: &FunctionInfo,
) -> Result<Option<SwiftFunction>> {
    if !func.name.starts_with(object.name()) {
        // Function is not assciated with the object.
        return Ok(None);
    }

    let mut ops = vec![];

    // Initalize the 'self' type, which is then passed on to the underlying
    // C FFI function, assuming the function is not static.
    //
    // E.g:
    // - `let obj = self.rawValue`
    // - `let obj = TWSomeEnum(rawValue: self.RawValue")`
    if !func.is_static {
        ops.push(match object {
            ObjectVariant::Struct(_) => SwiftOperation::Call {
                var_name: "obj".to_string(),
                call: "self.rawValue".to_string(),
                defer: None,
            },
            ObjectVariant::Enum(name) => SwiftOperation::Call {
                var_name: "obj".to_string(),
                call: format!("{}(rawValue: self.rawValue)", name),
                defer: None,
            },
        });
    }

    // For each parameter, we track a list of `params` which is used for the
    // function interface and add the necessary operations on how to process
    // those parameters.
    let mut params = vec![];
    for param in &func.params {
        // Skip self parameter
        match &param.ty.variant {
            TypeVariant::Enum(name) | TypeVariant::Struct(name) if name == object.name() => {
                continue
            }
            _ => {}
        }

        // Convert parameter to Swift parameter for the function interface.
        params.push(SwiftParam {
            name: param.name.clone(),
            param_type: SwiftType::from(param.ty.variant.clone()),
            is_nullable: param.ty.is_nullable,
        });

        // Process parameter.
        if let Some(op) = param_c_ffi_call(&param) {
            ops.push(op)
        }
    }

    // Prepepare parameter list to be passed on to the underlying C FFI function.
    let param_name = if func.is_static { vec![] } else { vec!["obj"] };
    let param_names = param_name
        .into_iter()
        .chain(params.iter().map(|p| p.name.as_str()))
        .collect::<Vec<&str>>()
        .join(",");

    // Call the underlying C FFI function, passing on the parameter list.
    let (var_name, call) = (
        "result".to_string(),
        format!("{}({})", func.name, param_names),
    );
    if func.return_type.is_nullable {
        ops.push(SwiftOperation::GuardedCall { var_name, call });
    } else {
        ops.push(SwiftOperation::Call {
            var_name,
            call,
            defer: None,
        });
    }

    // Wrap result.
    ops.push(wrap_return(&func.return_type));

    // Convert return type for function interface.
    let return_type = SwiftReturn {
        param_type: SwiftType::from(func.return_type.variant.clone()),
        is_nullable: func.return_type.is_nullable,
    };

    // Prettify name, remove object name prefix from this property.
    let pretty_name = pretty_object_method_name(&object, func.name.clone());

    Ok(Some(SwiftFunction {
        name: pretty_name,
        is_public: func.is_public,
        is_static: func.is_static,
        operations: ops,
        params,
        return_type,
        comments: vec![],
    }))
}
