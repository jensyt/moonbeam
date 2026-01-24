# Todo
- Support for HTML forms
- TRACE requests
- Make tracing meaningful
- Improve documentation (README and module/function documentation)
- Clean up project structure to remove example and test entries from Cargo.toml

# Done
- ETags for assets
- HEAD requests
- Default content-type and content-length
- Implement cookies
- Implement params
- Handle panics in server
- Content encoding (gzip, brotli)
- Better routing
- Support returning `impl Into<Response>` and `Result<impl Into<Response>, impl Into<Response>>`
  from routes
- Middleware support
- Route prefixes
- Automatic support for HEAD requests -> GET handler in router
