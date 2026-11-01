import { createServer } from 'node:http';
import { once } from 'node:events';
import { AgentError } from './runtime.mjs';

const json = (response, status, body) => { response.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store' }); response.end(JSON.stringify(body)); };
async function body(request) {
  let value = ''; for await (const chunk of request) { value += chunk; if (Buffer.byteLength(value) > 32768) throw new AgentError(413, 'too_large', 'Request too large.'); }
  try { return JSON.parse(value); } catch { throw new AgentError(400, 'invalid_request', 'Expected JSON object.'); }
}
export function agentServer(agent) {
  let streams = 0;
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, 'http://agent.internal');
      if (request.method === 'GET' && url.pathname === '/health/ready') return json(response, 200, { status: 'ok', configured: agent.configured });
      if (request.method === 'GET' && url.pathname === '/v1/agent') return json(response, 200, await agent.snapshot());
      if (request.method === 'GET' && url.pathname === '/v1/agent/history') return json(response, 200, await agent.history({ cursor: url.searchParams.get('cursor') ?? undefined, limit: Number(url.searchParams.get('limit') ?? 50) }));
      if (request.method === 'POST' && url.pathname === '/v1/agent/messages') return json(response, 202, await agent.submit(await body(request)));
      if (request.method === 'POST' && url.pathname === '/v1/agent/cancel') return json(response, 200, await agent.cancel());
      if (request.method === 'GET' && /^\/v1\/agent\/submissions\/[1-9][0-9]*$/.test(url.pathname)) {
        const id = Number(url.pathname.split('/').at(-1));
        if (!Number.isSafeInteger(id)) throw new AgentError(400, 'invalid_request', 'Invalid submission ID.');
        return json(response, 200, await agent.submission(id));
      }
      if (request.method === 'GET' && url.pathname === '/v1/agent/events') {
        if (streams >= 8) throw new AgentError(429, 'too_many_streams', 'Too many streams.');
        streams++;
        response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', 'x-accel-buffering': 'no' });
        response.flushHeaders();
        let stopped = false; let watch;
        const heartbeat = setInterval(() => { if (!response.destroyed) response.write(': keepalive\n\n'); }, 15000);
        response.once('close', () => { stopped = true; streams--; clearInterval(heartbeat); void watch?.stop(); });
        watch = await agent.watch(async snapshot => {
          if (stopped) return;
          const frame = `event: snapshot\ndata: ${JSON.stringify(snapshot)}\n\n`;
          if (!response.write(frame)) await Promise.race([once(response, 'drain'), once(response, 'close')]);
        });
        if (stopped) await watch.stop();
        return;
      }
      json(response, 404, { error: { code: 'not_found', message: 'Route not found.' } });
    } catch (error) {
      if (response.headersSent) { response.destroy(); return; }
      const status = error.status ?? (error.name === 'ConversationBusy' ? 409 : 500);
      json(response, status, { error: { code: error.code ?? (status === 409 ? 'busy' : 'internal_error'), message: status < 500 || status === 503 ? error.message : 'Agent operation failed.' } });
    }
  });
  server.requestTimeout = 10000;
  server.headersTimeout = 10000;
  return server;
}
