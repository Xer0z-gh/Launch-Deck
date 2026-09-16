// Fixture for conflict-verify: a node project whose runner guesses port
// 3000, so the two-tier port gate has something real to warn about.
require('node:http').createServer((_, res) => res.end('ok')).listen(3000);
