use quote::quote;
use syn::{FnArg, ItemFn, PathArguments, Type, parse_macro_input, parse_quote};

pub fn middleware_impl(
	_attr: proc_macro::TokenStream,
	item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
	let mut input_fn = parse_macro_input!(item as ItemFn);

	// 1. Inject generics <'a, 'b, Fut>
	input_fn.sig.generics.params.push(parse_quote!('a));
	input_fn.sig.generics.params.push(parse_quote!('b));
	input_fn.sig.generics.params.push(parse_quote!(Fut));

	// 2. Add where clause: where Fut: Future<Output = Response>
	let where_clause = input_fn.sig.generics.make_where_clause();
	where_clause
		.predicates
		.push(parse_quote!(Fut: ::std::future::Future<Output = ::moonbeam::Response>));

	// 3. Process arguments
	for arg in &mut input_fn.sig.inputs {
		if let FnArg::Typed(pat_type) = arg {
			// Check arg name to guess purpose, or type
			// We look for specific types to replace.

			if let Type::Path(tp) = &mut *pat_type.ty {
				if tp
					.path
					.segments
					.last()
					.map(|s| s.ident == "Request" && s.arguments.is_empty())
					.unwrap_or(false)
				{
					// Add <'a, 'b>
					tp.path.segments.last_mut().unwrap().arguments =
						PathArguments::AngleBracketed(parse_quote!(<'a, 'b>));
				} else if tp
					.path
					.segments
					.last()
					.map(|s| s.ident == "Next")
					.unwrap_or(false)
				{
					// Replace Next with impl FnOnce(Request<'a, 'b>) -> Fut
					pat_type.ty = parse_quote!(impl FnOnce(::moonbeam::Request<'a, 'b>) -> Fut);
				}
			}
		}
	}

	// 4. Ensure return type is Response
	// (Optional: we could allow impl Into<Response> if we adjust the Fut bound too)

	quote! {
		#input_fn
	}
	.into()
}
