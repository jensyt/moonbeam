const http = require('http');

const server = http.createServer((req, res) => {
	res.writeHead(200, { 'Content-Type': 'text/plain' });
	res.end('Hello, World!');
});

const PORT = 3030;
server.listen(PORT, '127.0.0.1', () => {
	console.log(`Node.js server listening on http://127.0.0.1:${PORT}/`);
});
