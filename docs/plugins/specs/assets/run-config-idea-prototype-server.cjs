/** Serve only the disposable design page on loopback; no project files or write APIs are exposed.
 * Run: node docs/plugins/specs/assets/run-config-idea-prototype-server.cjs [port]
 * Omitting the port selects an available local port and prints the resulting preview URL. */
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, 'run-config-idea-prototype.html'));
const server = http.createServer((request, response) => {
  const route = new URL(request.url, 'http://localhost').pathname;
  if (route !== '/' && route !== '/run-config-idea-prototype.html') {
    response.writeHead(404).end();
    return;
  }
  response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
  response.end(source);
});
server.listen(Number(process.argv[2] || 0), '127.0.0.1', () => {
  process.stdout.write(`Design preview: http://127.0.0.1:${server.address().port}/?variant=A\n`);
});
