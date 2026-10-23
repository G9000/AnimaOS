import { spawn } from 'node:child_process';
const port = process.argv[2];

// Exercise a real Bun descendant without coupling process cleanup to a Rust
// compilation or a user's daemon profile.
spawn(
  'bun',
  [
    '-e',
    `Bun.serve({ hostname: '127.0.0.1', port: ${Number(port)},
  fetch() { return new Response('ok'); } });`,
  ],
  {
    env: process.env,
    stdio: 'ignore',
  },
);

const holdOpen = setInterval(() => {}, 1000);

const shutdown = () => {
  clearInterval(holdOpen);
  process.exit(0);
};

process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
