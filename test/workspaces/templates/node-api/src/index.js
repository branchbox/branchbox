const http = require("http");

const port = process.env.PORT || 3000;

const server = http.createServer((_req, res) => {
  res.statusCode = 200;
  res.setHeader("Content-Type", "text/plain");
  res.end("BranchBox devcontainer smoke test\n");
});

server.listen(port, () => {
  console.log(`Smoke test server running at http://localhost:${port}/`);
});
