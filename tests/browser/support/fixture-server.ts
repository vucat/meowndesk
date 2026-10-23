import { createServer } from 'vite';

// Separate lifecycle/port for tests. Telemetry fixtures are added in T019.
const server = await createServer({
  server: { host: '127.0.0.1', port: 1421, strictPort: true },
});
await server.listen();

for (const signal of ['SIGTERM', 'SIGINT'] as const) {
  process.once(signal, () => {
    void server.close().then(() => process.exit(0));
  });
}
