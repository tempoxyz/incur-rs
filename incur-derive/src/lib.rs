use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Lit, Meta, parse_macro_input};

/// Derive macro for defining incur CLI commands from structs.
///
/// # Struct-level attributes
///
/// - `#[incur(name = "...")]` — CLI name (defaults to lowercase struct name)
/// - `#[incur(description = "...")]` — CLI description
/// - `#[incur(version = "...")]` — CLI version
///
/// # Field-level attributes
///
/// - `#[incur(arg)]` — positional argument (default is named option)
/// - `#[incur(required)]` — mark as required
/// - `#[incur(short = 'x')]` — short flag for options
/// - `#[incur(default = "value")]` — default value
/// - `#[incur(description = "...")]` — description (also inferred from `///` doc comments)
/// - `#[incur(enum_values = ["a", "b"])]` — constrain to specific values
#[proc_macro_derive(Incur, attributes(incur))]
pub fn derive_incur(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_incur(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

struct StructAttrs {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
}

struct FieldAttrs {
    is_arg: bool,
    required: bool,
    short: Option<char>,
    default: Option<String>,
    description: Option<String>,
    enum_values: Vec<String>,
    env: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum FieldKind {
    /// `String`
    String,
    /// `bool`
    Bool,
    /// Numeric: f64, f32, i32, i64, u32, u64, usize
    Number,
    /// `Vec<String>`
    Array,
}

#[derive(Clone, Copy, PartialEq)]
enum Wrapper {
    None,
    Option,
}

fn parse_struct_attrs(input: &DeriveInput) -> syn::Result<StructAttrs> {
    let mut attrs = StructAttrs {
        name: None,
        description: None,
        version: None,
    };

    for attr in &input.attrs {
        if !attr.path().is_ident("incur") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    attrs.name = Some(s.value());
                }
            } else if meta.path.is_ident("description") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    attrs.description = Some(s.value());
                }
            } else if meta.path.is_ident("version") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    attrs.version = Some(s.value());
                }
            }
            Ok(())
        })?;
    }

    Ok(attrs)
}

fn parse_field_attrs(field: &syn::Field) -> syn::Result<FieldAttrs> {
    let mut attrs = FieldAttrs {
        is_arg: false,
        required: false,
        short: None,
        default: None,
        description: None,
        enum_values: Vec::new(),
        env: None,
    };

    // Extract doc comments as description fallback.
    for attr in &field.attrs {
        if attr.path().is_ident("doc") {
            if let Meta::NameValue(nv) = &attr.meta {
                if let syn::Expr::Lit(expr_lit) = &nv.value {
                    if let Lit::Str(s) = &expr_lit.lit {
                        let text = s.value();
                        let text = text.trim();
                        if attrs.description.is_none() {
                            attrs.description = Some(text.to_string());
                        } else {
                            let prev = attrs.description.take().unwrap();
                            attrs.description = Some(format!("{prev} {text}"));
                        }
                    }
                }
            }
        }
    }

    for attr in &field.attrs {
        if !attr.path().is_ident("incur") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("arg") {
                attrs.is_arg = true;
            } else if meta.path.is_ident("option") {
                // explicit option marker (default behavior)
            } else if meta.path.is_ident("required") {
                attrs.required = true;
            } else if meta.path.is_ident("short") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Char(c) = lit {
                    attrs.short = Some(c.value());
                }
            } else if meta.path.is_ident("default") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    attrs.default = Some(s.value());
                }
            } else if meta.path.is_ident("description") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    attrs.description = Some(s.value());
                }
            } else if meta.path.is_ident("enum_values") {
                let value = meta.value()?;
                let content;
                syn::bracketed!(content in value);
                let values =
                    syn::punctuated::Punctuated::<Lit, syn::Token![,]>::parse_terminated(&content)?;
                for lit in values {
                    if let Lit::Str(s) = lit {
                        attrs.enum_values.push(s.value());
                    }
                }
            } else if meta.path.is_ident("env") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    attrs.env = Some(s.value());
                }
            }
            Ok(())
        })?;
    }

    Ok(attrs)
}

/// Determine the `FieldKind` and whether it's wrapped in `Option<T>`.
fn classify_type(ty: &syn::Type) -> (FieldKind, Wrapper) {
    if let syn::Type::Path(tp) = ty {
        let seg = &tp.path.segments;
        if let Some(last) = seg.last() {
            let ident = last.ident.to_string();

            // Check for Option<T>
            if ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        let (kind, _) = classify_type(inner);
                        return (kind, Wrapper::Option);
                    }
                }
                return (FieldKind::String, Wrapper::Option);
            }

            // Check for Vec<String>
            if ident == "Vec" {
                return (FieldKind::Array, Wrapper::None);
            }

            match ident.as_str() {
                "bool" => return (FieldKind::Bool, Wrapper::None),
                "f64" | "f32" | "i32" | "i64" | "u32" | "u64" | "usize" | "isize" => {
                    return (FieldKind::Number, Wrapper::None);
                }
                "String" => return (FieldKind::String, Wrapper::None),
                _ => {}
            }
        }
    }
    (FieldKind::String, Wrapper::None)
}

fn impl_incur(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let struct_name = &input.ident;
    let struct_attrs = parse_struct_attrs(input)?;

    let cli_name = struct_attrs
        .name
        .unwrap_or_else(|| struct_name.to_string().to_lowercase());
    let cli_desc = match struct_attrs.description {
        Some(d) => quote! { ::core::option::Option::Some(#d) },
        None => quote! { ::core::option::Option::None },
    };
    let cli_ver = match struct_attrs.version {
        Some(v) => quote! { ::core::option::Option::Some(#v) },
        None => quote! { ::core::option::Option::None },
    };

    let fields = match &input.data {
        Data::Struct(ds) => match &ds.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    input,
                    "Incur can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "Incur can only be derived for structs",
            ));
        }
    };

    let mut arg_tokens = Vec::new();
    let mut opt_tokens = Vec::new();
    let mut from_ctx_tokens = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        let field_attrs = parse_field_attrs(field)?;
        let (kind, wrapper) = classify_type(&field.ty);

        let desc = &field_attrs.description;

        if field_attrs.is_arg {
            // --- Positional argument ---
            let required = field_attrs.required && wrapper == Wrapper::None;
            let mut builder = quote! {
                ::incur::Arg::new(#field_name_str)
            };
            if let Some(d) = desc {
                builder = quote! { #builder.description(#d) };
            }
            if required {
                builder = quote! { #builder.required(true) };
            }
            if let Some(def) = &field_attrs.default {
                builder = quote! { #builder.default_value(#def) };
            }
            arg_tokens.push(builder);

            // from_context extraction
            match wrapper {
                Wrapper::Option => {
                    from_ctx_tokens.push(quote! {
                        #field_name: ctx.arg_opt::<String>(#field_name_str),
                    });
                }
                Wrapper::None => match kind {
                    FieldKind::String => {
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.arg::<String>(#field_name_str),
                        });
                    }
                    FieldKind::Number => {
                        let ty = &field.ty;
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.arg::<#ty>(#field_name_str),
                        });
                    }
                    FieldKind::Bool => {
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.arg::<bool>(#field_name_str),
                        });
                    }
                    FieldKind::Array => {
                        // Positional arrays don't make much sense, treat as string
                        from_ctx_tokens.push(quote! {
                            #field_name: vec![ctx.arg::<String>(#field_name_str)],
                        });
                    }
                },
            }
        } else {
            // --- Named option ---
            let mut builder = quote! {
                ::incur::Opt::new(#field_name_str)
            };
            if let Some(d) = desc {
                builder = quote! { #builder.description(#d) };
            }
            if let Some(c) = field_attrs.short {
                builder = quote! { #builder.short(#c) };
            }
            match kind {
                FieldKind::Bool => {
                    builder = quote! { #builder.boolean() };
                }
                FieldKind::Number => {
                    builder = quote! { #builder.number() };
                }
                FieldKind::Array => {
                    builder = quote! { #builder.array() };
                }
                FieldKind::String => {}
            }
            if field_attrs.required && wrapper == Wrapper::None {
                builder = quote! { #builder.required(true) };
            }
            if let Some(def) = &field_attrs.default {
                builder = quote! { #builder.default_value(#def) };
            }
            if !field_attrs.enum_values.is_empty() {
                let vals = &field_attrs.enum_values;
                builder = quote! { #builder.enum_values([#(#vals),*]) };
            }
            if let Some(env_name) = &field_attrs.env {
                builder = quote! { #builder.env(#env_name) };
            }
            opt_tokens.push(builder);

            // from_context extraction
            match wrapper {
                Wrapper::Option => match kind {
                    FieldKind::String => {
                        from_ctx_tokens.push(quote! {
                            #field_name: {
                                let s = ctx.option_str(#field_name_str);
                                if s.is_empty() { None } else { Some(s.to_string()) }
                            },
                        });
                    }
                    FieldKind::Number => {
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.option(#field_name_str).map(|v| v.as_f64() as _),
                        });
                    }
                    FieldKind::Bool => {
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.option(#field_name_str).map(|v| v.as_bool()),
                        });
                    }
                    FieldKind::Array => {
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.option(#field_name_str).map(|v| {
                                match v {
                                    ::incur::parser::Value::Array(a) => a.clone(),
                                    _ => vec![v.as_str().to_string()],
                                }
                            }),
                        });
                    }
                },
                Wrapper::None => match kind {
                    FieldKind::Bool => {
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.option_bool(#field_name_str),
                        });
                    }
                    FieldKind::Number => {
                        let ty = &field.ty;
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.option_f64(#field_name_str) as #ty,
                        });
                    }
                    FieldKind::String => {
                        from_ctx_tokens.push(quote! {
                            #field_name: ctx.option_str(#field_name_str).to_string(),
                        });
                    }
                    FieldKind::Array => {
                        from_ctx_tokens.push(quote! {
                            #field_name: match ctx.option(#field_name_str) {
                                Some(::incur::parser::Value::Array(a)) => a.clone(),
                                Some(v) => vec![v.as_str().to_string()],
                                None => Vec::new(),
                            },
                        });
                    }
                },
            }
        }
    }

    let expanded = quote! {
        impl ::incur::IncurCommand for #struct_name {
            fn incur_args() -> ::std::vec::Vec<::incur::Arg> {
                vec![#(#arg_tokens),*]
            }

            fn incur_options() -> ::std::vec::Vec<::incur::Opt> {
                vec![#(#opt_tokens),*]
            }

            fn from_context(ctx: &::incur::CommandContext) -> Self {
                Self {
                    #(#from_ctx_tokens)*
                }
            }

            fn cli_name() -> &'static str {
                #cli_name
            }

            fn cli_description() -> ::core::option::Option<&'static str> {
                #cli_desc
            }

            fn cli_version() -> ::core::option::Option<&'static str> {
                #cli_ver
            }
        }
    };

    Ok(expanded)
}
