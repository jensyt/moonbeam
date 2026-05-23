use quote::quote;
use syn::{FnArg, ItemFn, PathArguments, Type, parse_macro_input, parse_quote, spanned::Spanned};

pub(super) fn middleware_impl(
	_attr: proc_macro::TokenStream,
	item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
	let mut input_fn = parse_macro_input!(item as ItemFn);

	// 1. Inject generics <'a, 'b, 'e, Fut>
	input_fn.sig.generics.params.push(parse_quote!('a));
	input_fn.sig.generics.params.push(parse_quote!('b));
	input_fn.sig.generics.params.push(parse_quote!('e));
	input_fn.sig.generics.params.push(parse_quote!(Fut));

	// 2. Add where clause: where Fut: Future<Output = Response>
	let where_clause = input_fn.sig.generics.make_where_clause();
	where_clause
		.predicates
		.push(parse_quote!(Fut: ::std::future::Future<Output = ::moonbeam::Response>));

	// 3. Process arguments
	let inputs = &mut input_fn.sig.inputs;
	if inputs.len() != 4 {
		return syn::Error::new(
			inputs.span(),
			"Middleware must have exactly 4 arguments: (Request, Spawner, State, Next)",
		)
		.to_compile_error()
		.into();
	}

	let mut inputs_iter = inputs.iter_mut();

	// Arg 1: Request
	let arg1 = inputs_iter.next().unwrap();
	match extract_path_segment(arg1, "First", "Request") {
		Ok(seg) => {
			if seg.arguments.is_empty() {
				seg.arguments = PathArguments::AngleBracketed(parse_quote!(<'a, 'b>));
			}
		}
		Err(err) => return err.to_compile_error().into(),
	}

	// Arg 2: Spawner
	let arg2 = inputs_iter.next().unwrap();
	match extract_path_segment(arg2, "Second", "Spawner") {
		Ok(seg) => {
			if seg.arguments.is_empty() {
				seg.arguments = PathArguments::AngleBracketed(parse_quote!(<'e>));
			}
		}
		Err(err) => return err.to_compile_error().into(),
	}

	// Arg 3: State
	let arg3 = inputs_iter.next().unwrap();
	match arg3 {
		FnArg::Typed(pat_type) => {
			if !matches!(*pat_type.ty, Type::Reference(_)) {
				return syn::Error::new(
					pat_type.ty.span(),
					"Third argument State must be a reference",
				)
				.to_compile_error()
				.into();
			}
		}
		FnArg::Receiver(recv) => {
			return syn::Error::new(recv.span(), "Third argument must be State, not self")
				.to_compile_error()
				.into();
		}
	}

	// Arg 4: Next
	let arg4 = inputs_iter.next().unwrap();
	match extract_path_segment(arg4, "Fourth", "Next") {
		Ok(_) => {
			if let FnArg::Typed(pat_type) = arg4 {
				pat_type.ty = parse_quote!(impl FnOnce(::moonbeam::Request<'a, 'b>) -> Fut);
			}
		}
		Err(err) => return err.to_compile_error().into(),
	}

	quote! {
		#input_fn
	}
	.into()
}

/// Helper function to extract a mutable reference to the last segment of a type path
/// and validate that the type matches the expected identifier name.
fn extract_path_segment<'a>(
	arg: &'a mut FnArg,
	arg_name: &str,
	expected: &str,
) -> Result<&'a mut syn::PathSegment, syn::Error> {
	let typed = match arg {
		FnArg::Typed(t) => t,
		FnArg::Receiver(r) => {
			return Err(syn::Error::new(
				r.span(),
				format!("{} argument must be {}, not self", arg_name, expected),
			));
		}
	};
	let path_type = match &mut *typed.ty {
		Type::Path(p) => p,
		other => {
			return Err(syn::Error::new(
				other.span(),
				format!("{} argument must be {}", arg_name, expected),
			));
		}
	};
	let type_span = path_type.span();
	let last = path_type.path.segments.last_mut().ok_or_else(|| {
		syn::Error::new(
			type_span,
			format!("{} argument must be {}", arg_name, expected),
		)
	})?;
	if last.ident != expected {
		return Err(syn::Error::new(
			last.span(),
			format!("{} argument must be {}", arg_name, expected),
		));
	}
	Ok(last)
}
