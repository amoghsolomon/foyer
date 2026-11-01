import { openAgent, modelsFromEnvironment } from './runtime.mjs';
import { agentServer } from './http.mjs';

process.umask(0o077);
const directory = process.env.FOYER_AGENT_DATA_DIR ?? '/data/agent';
const agent = await openAgent({ directory, ...await modelsFromEnvironment() });
const server = agentServer(agent);
const host = process.env.FOYER_AGENT_HOST ?? '127.0.0.1';
const port = Number(process.env.FOYER_AGENT_PORT ?? 3584);
server.listen(port, host, () => console.log(JSON.stringify({ event: 'agent_ready', configured: agent.configured })));
let closing = false;
async function close() {
  if (closing) return; closing = true;
  server.close(); server.closeAllConnections();
  await agent.close();
}
process.once('SIGTERM', () => { void close(); });
process.once('SIGINT', () => { void close(); });
