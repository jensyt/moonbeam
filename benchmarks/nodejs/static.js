const http = require('http');
const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, '..', 'static', 'test.txt');

const server = http.createServer((req, res) => {
	// Simple static file server (for benchmarking)
	fs.readFile(filePath, (err, data) => {
		if (err) {
			res.writeHead(500);
			res.end('Error');
			return;
		}
		res.writeHead(200, { 'Content-Type': 'text/plain' });
		res.end(data);
	});
});

const PORT = 3030;
server.listen(PORT, '127.0.0.1', () => {
	console.log(`Node.js Static server listening on http://127.0.0.1:${PORT}/`);
});
