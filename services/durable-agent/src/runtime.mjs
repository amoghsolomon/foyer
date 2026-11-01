import { mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import lockfile from 'proper-lockfile';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { createModels } from '@earendil-works/pi-ai';
import { Harness, createRegistry, defineDoc } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';

export class AgentError extends Error {
  constructor(status, code, message) { super(message); this.status = status; this.code = code; }
}
const textOf = content => typeof content === 'string' ? content : (content ?? []).filter(item => item.type === 'text').map(item => item.text).join('');
export function publicEntry(entry) {
  const messages = (entry.model ?? []).filter(message => message.role === 'user' || message.role === 'assistant');
  if (!messages.length) return undefined;
  return { id: entry.id, messages: messages.map(message => ({ role: message.role, text: textOf(message.content) })) };
}
export function publicView(view, configured) {
  const live = view.docs['pi.live'] ?? {};
  return { threadId: 'main', configured, busy: Boolean(live.run),
    entries: view.entries.slice(-100).map(publicEntry).filter(Boolean),
    partial: textOf(live.generation?.message?.content),
    retryAt: live.generation?.retry?.at ?? null };
}

const Admissions = defineDoc({ kind: 'foyer.admissions', version: 1, scope: 'conversation',
  history: 'latest', fork: 'initial', initial: () => ({ requests: {} }), checkpointWhen: () => true });
const publicStatus = record => ({ state: record.status, ...(record.answer === undefined ? {} : { answerId: record.answer }),
  ...(record.reason === undefined ? {} : { reason: record.reason }) });

export async function modelsFromEnvironment(env = process.env) {
  const models = createModels();
  const provider = env.FOYER_AGENT_PROVIDER;
  const modelId = env.FOYER_AGENT_MODEL;
  if (!provider && !modelId) return { models, model: undefined };
  const providers = {
    openai: ['OPENAI_API_KEY', () => import('@earendil-works/pi-ai/providers/openai').then(m => m.openaiProvider())],
    anthropic: ['ANTHROPIC_API_KEY', () => import('@earendil-works/pi-ai/providers/anthropic').then(m => m.anthropicProvider())],
    openrouter: ['OPENROUTER_API_KEY', () => import('@earendil-works/pi-ai/providers/openrouter').then(m => m.openrouterProvider())],
  };
  const selected = providers[provider];
  if (!selected || !modelId || !env[selected[0]]) throw new Error('Provider, model and its private API key must all be configured.');
  models.setProvider(await selected[1]());
  return { models, model: { provider, modelId } };
}

export async function openAgent({ directory, models, model, lockRetries = 30 }) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const release = await lockfile.lock(join(directory, 'thread.sqlite'), { realpath: false, stale: 10000, update: 2000,
    lockfilePath: join(directory, 'writer.lock'), retries: { retries: lockRetries, factor: 1, minTimeout: 500, maxTimeout: 500 } });
  let harness;
  try {
    harness = await Harness.open(await openNodeSqliteStorage(join(directory, 'thread.sqlite')), {
      models, registry: createRegistry(), settings: { retry: { maxRetries: 2 },
        // No tools/extensions/environment, no subagents, memory, or schedules.
        compaction: { enabled: false } },
      onReport: report => { console.error(JSON.stringify({ event: 'durable_report', kind: report.kind })); },
    }, context);
    const root = await harness.root(context, { agent: { ...(model ? { model } : {}), tools: [], extensions: [] } });
    await root.configure({ model: model ?? null, tools: [], extensions: [] }, context);
    if (model) harness.resume(); // Opening an unconfigured service never invokes a provider.
    let closed = false;
    let admissionLine = Promise.resolve();
    return {
      harness, root, configured: Boolean(model),
      async snapshot() { const view = await root.viewState(context); try { return publicView(view.value, Boolean(model)); } finally { view.dispose(); } },
      async history({ cursor, limit = 50 } = {}) {
        if (!Number.isInteger(limit) || limit < 1 || limit > 100) throw new AgentError(400, 'invalid_request', 'limit must be 1..100.');
        const page = await root.entries({ order: 'descending' }, limit, cursor, context);
        return { threadId: 'main', entries: page.items.map(publicEntry).filter(Boolean), nextCursor: page.next ?? null };
      },
      async submit(input) {
        if (!input || typeof input !== 'object' || Array.isArray(input)) throw new AgentError(400, 'invalid_request', 'Expected a message object.');
        const { requestId, content } = input;
        if (!model) throw new AgentError(503, 'provider_not_configured', 'The model provider is not configured.');
        if (typeof requestId !== 'string' || !/^[A-Za-z0-9._:-]{1,128}$/.test(requestId) || typeof content !== 'string' || !content.trim() || content.length > 16000)
          throw new AgentError(400, 'invalid_request', 'A requestId and 1..16000 characters of content are required.');
        const current = admissionLine.then(async () => {
          const hash = createHash('sha256').update(content).digest('hex');
          // The published 1.4.0 SDK does not admit inside a host commit. Persist the binding
          // first; a crash between admission and the receipt is recovered by Pi's requestId.
          const previousId = await root.commit(async tx => {
            const admissions = await tx.doc(Admissions, root.id);
            const previous = admissions.requests[requestId];
            if (previous) {
              if (previous.hash !== hash) throw new AgentError(409, 'request_conflict', 'requestId is bound to different content.');
              return previous.id;
            }
            if (Object.keys(admissions.requests).length >= 10000) throw new AgentError(429, 'thread_limit', 'Thread admission limit reached.');
            admissions.requests[requestId] = { hash };
          }, context);
          const submission = previousId === undefined
            ? await root.submit({ type: 'input', content, requestId, whenBusy: 'reject' }, context)
            : await harness.submission(previousId, context);
          await root.commit(async tx => { (await tx.doc(Admissions, root.id)).requests[requestId].id = submission.id; }, context);
          return { threadId: 'main', submissionId: submission.id, status: publicStatus(await submission.status(context)) };
        });
        admissionLine = current.catch(() => {});
        return current;
      },

      async submission(id) {
        const submission = await harness.submission(id, context);
        if (!submission) throw new AgentError(404, 'not_found', 'Submission not found.');
        return { threadId: 'main', submissionId: id, status: publicStatus(await submission.status(context)) };
      },
      async cancel() { await root.abort(context); return { threadId: 'main', cancelled: true }; },
      async watch(listener) { const watch = await root.watch(context); await listener(publicView(watch.value, Boolean(model))); watch.start(value => listener(publicView(value, Boolean(model)))); return watch; },
      async close() { if (closed) return; closed = true; try { await harness.close(context); } finally { await release(); } },
    };
  } catch (error) { if (harness) await harness.close(context); await release(); throw error; }
}
