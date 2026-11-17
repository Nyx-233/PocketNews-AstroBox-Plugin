// Keep the companion Vela app in ../news and pipe `cargo run --quiet --example fixture -- --hundred` into this script.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../../news/src/common/protocol.js', import.meta.url), 'utf8');
const { createReceiver, control, MAX_ARTICLES, MAX_BYTES } = await import(
  'data:text/javascript;base64,' + Buffer.from(source).toString('base64')
);
let input = '';
process.stdin.setEncoding('utf8');
for await (const part of process.stdin) input += part;
const { packets, batch } = JSON.parse(input);
assert.equal(MAX_ARTICLES, 100);
assert.equal(batch.articles.length, MAX_ARTICLES);
assert.ok(batch.articles.every(article => article.content === undefined), 'Batches are text-only');
assert.ok(packets.every(packet => packet.length <= 18000));
const header = JSON.parse(packets[1].slice(1));
assert.ok(header.totalBytes <= MAX_BYTES);
const frames = packets.filter(packet => packet.startsWith('F'));
assert.ok(frames.length > 4, 'Fixture must exercise more than one transport window');

let metadata = null, received = [];
const bytes = Buffer.alloc(header.totalBytes), replies = [], saved = [];
const storage = {
  restore: async m => metadata && metadata.themeId === m.themeId
    ? { ...metadata, received: received.slice() } : null,
  begin: async m => { metadata = { themeId: m.themeId, totalBytes: m.totalBytes }; received = []; bytes.fill(0); },
  prepare: async m => { metadata.chunkSizeBytes = m.chunkSizeBytes; received = new Array(m.chunkCount).fill(false); },
  append: async p => { bytes.set(p.data, p.position); received[p.seq] = true; },
  read: async () => bytes.toString('utf8'),
  completed: id => saved.some(item => item.id === id),
  save: async value => { saved.push(value); },
  send: packet => replies.push(packet),
  progress: () => {}
};
async function send(receiver, list) {
  for (const packet of list) await receiver.receive(packet);
}
const first = createReceiver(storage);
await send(first, packets.slice(0, 5));
const halfway = Math.floor(frames.length / 2);
await send(first, frames.slice(0, halfway));
assert.equal(saved.length, 0);
first.reset();

const resumed = createReceiver(storage);
const resumeManifest = packets.slice(0, 5).map(packet => packet.startsWith('T') && JSON.parse(packet.slice(1)).operation === 'begin'
  ? control('T', { ...header, mode: 'resume' }) : packet);
await send(resumed, resumeManifest);
const checkpoint = JSON.parse(replies.at(-1).slice(1));
assert.equal(checkpoint.status, 'resume');
assert.deepEqual(checkpoint.receivedRanges, [[0, halfway - 1]]);
await send(resumed, frames.slice(halfway));
await resumed.receive(packets.at(-2));
assert.equal(replies.at(-1), 'C0000');
assert.equal(saved.length, 0, 'File completion is not a durable news commit');
await resumed.receive(packets.at(-1));
assert.deepEqual(saved, [batch]);
assert.equal(JSON.parse(replies.at(-1).slice(1)).status, 'ready');
await resumed.receive(packets.at(-1));
assert.equal(saved.length, 1, 'A repeated finish must not duplicate the batch');
console.log(`PASS: Rust → JS receiver, ${batch.articles.length} text articles; ${header.totalBytes} total bytes, ${frames.length} chunks; restart/resume and final commit verified.`);
