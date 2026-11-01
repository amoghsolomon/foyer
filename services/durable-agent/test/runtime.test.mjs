import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fork } from 'node:child_process';
import { once } from 'node:events';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { createModels, fauxProvider, fauxAssistantMessage } from '@earendil-works/pi-ai';
import { openAgent, publicEntry, modelsFromEnvironment } from '../src/runtime.mjs';
import { agentServer } from '../src/http.mjs';

async function directory(t) { const path = await mkdtemp(join(tmpdir(), 'foyer-agent-')); t.after(() => rm(path, { recursive: true, force: true })); return path; }
function setup(steps = [fauxAssistantMessage('Hello from durable Pi.')]) {
  const faux = fauxProvider(); faux.setResponses(steps); const models = createModels(); models.setProvider(faux.provider);
  return { models, model: { provider: 'faux', modelId: 'faux-1' } };
}
async function open(t, path, options = setup()) { const agent = await openAgent({ directory: path, ...options }); t.after(() => agent.close()); return agent; }
async function settled(agent, id) { return (await agent.harness.submission(id, context)).wait(context); }

test('unconfigured is explicit, no input admitted, and one writer owns the store', async t => {
  const path = await directory(t); const agent = await open(t, path, await modelsFromEnvironment({}));
  assert.equal((await agent.snapshot()).configured, false);
  await assert.rejects(agent.submit({requestId:'a',content:'hello'}), error => error.code === 'provider_not_configured');
  assert.deepEqual((await agent.history()).entries, []);
  await assert.rejects(openAgent({ directory: path, models: createModels(), lockRetries: 0 }), error => error.code === 'ELOCKED');
});

test('committed answer, request binding and history survive reopen', async t => {
  const path = await directory(t); const agent = await open(t, path);
  const first = await agent.submit({ requestId: 'request-1', content: 'Hello' });
  assert.equal((await settled(agent, first.submissionId)).status, 'done');
  assert.equal((await agent.submit({ requestId:'request-1',content:'Hello' })).submissionId, first.submissionId);
  await assert.rejects(agent.submit({requestId:'request-1',content:'Changed'}), error => error.code === 'request_conflict');
  const before = (await agent.history()).entries;
  assert.equal(before.length, 2); assert.equal(before[0].messages[0].text, 'Hello from durable Pi.');
  await agent.close();
  const reopened = await open(t, path);
  assert.deepEqual((await reopened.history()).entries, before);
  assert.equal((await reopened.submit({ requestId:'request-1',content:'Hello' })).submissionId, first.submissionId);
  assert.equal((await reopened.submission(first.submissionId)).status.state, 'done');
  const page = await reopened.history({limit:1}); assert.equal(page.entries.length,1); assert.ok(page.nextCursor);
  assert.equal((await reopened.history({limit:1,cursor:page.nextCursor})).entries.length,1);
});

test('single busy turn rejects another input and cancel settles the original', async t => {
  let reached; const started = new Promise(resolve => { reached = resolve; });
  const hanging = (_ctx, options) => new Promise((_, reject) => { reached(); options.signal.addEventListener('abort', () => reject(options.signal.reason), {once:true}); });
  const agent = await open(t, await directory(t), setup([hanging]));
  const first = await agent.submit({requestId:'one',content:'wait'}); await started;
  await assert.rejects(agent.submit({requestId:'two',content:'other'}), error => error.name === 'ConversationBusy');
  assert.equal((await agent.snapshot()).busy,true);
  await agent.cancel(); assert.equal((await settled(agent,first.submissionId)).status,'unanswered');
  assert.equal((await agent.snapshot()).busy,false);
});

test('public projection excludes thinking and system prompts', () => {
  assert.equal(publicEntry({id:1,model:[{role:'system',content:'private prompt'}]}),undefined);
  assert.deepEqual(publicEntry({id:2,model:[{role:'assistant',content:[{type:'thinking',thinking:'private reasoning'},{type:'text',text:'public answer'}]}]}),{id:2,messages:[{role:'assistant',text:'public answer'}]});
});

test('HTTP contract exposes setup state, bounded errors and reconnectable snapshots', async t => {
  const agent = await open(t, await directory(t), await modelsFromEnvironment({})); const server = agentServer(agent);
  server.listen(0,'127.0.0.1'); await once(server,'listening'); t.after(() => {server.closeAllConnections(); server.close();});
  const base = `http://127.0.0.1:${server.address().port}`;
  assert.equal((await fetch(base+'/health/ready')).status,200);
  let response=await fetch(base+'/v1/agent/messages',{method:'POST',body:JSON.stringify({requestId:'a',content:'Hi'})});
  assert.equal(response.status,503); assert.equal((await response.json()).error.code,'provider_not_configured');
  assert.equal((await fetch(base+'/v1/agent/history?limit=1000')).status,400);
  for (let i=0;i<2;i++) {
    const controller=new AbortController(); response=await fetch(base+'/v1/agent/events',{signal:controller.signal});
    assert.equal(response.headers.get('content-type'),'text/event-stream');
    const reader=response.body.getReader(); const chunk=await reader.read();
    assert.match(new TextDecoder().decode(chunk.value),/event: snapshot/); controller.abort(); await reader.cancel().catch(()=>{});
  }
});

test('a killed worker resumes the admitted turn using the same persistent thread', {timeout:30000}, async t => {
  const path=await directory(t); const child=fork(new URL('../fixtures/crash-host.mjs',import.meta.url),[path],{stdio:['ignore','ignore','ignore','ipc']});
  t.after(()=>child.kill('SIGKILL')); const [message]=await once(child,'message'); assert.ok(message.id);
  child.kill('SIGKILL'); await once(child,'exit');
  const agent=await open(t,path,setup([fauxAssistantMessage('Recovered answer.')]));
  assert.equal((await settled(agent,message.id)).status,'done');
  assert.equal((await agent.history()).entries[0].messages[0].text,'Recovered answer.');
  assert.equal((await agent.submit({requestId:'crash-input',content:'Survive restart'})).submissionId,message.id);
});
