const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const context = vm.createContext({ window: {} });
vm.runInContext(fs.readFileSync('ui/runtime.js', 'utf8'), context);
const Runtime = context.window.FormsRuntime;

test('Oracle Run requires credentials when server credential use is not selected', () => {
  for (const backend of ['oracle', 'oracle-thin']) {
    assert.equal(Runtime.requiresCredentials({ backend }), true);
    assert.equal(Runtime.requiresCredentials({ backend, username: '   ', password: 'test' }), true);
    assert.equal(Runtime.requiresCredentials({ backend, username: 'test' }), true);
    assert.equal(Runtime.requiresCredentials({ backend, username: 'test', password: 'test' }), false);
  }
});
test('offline backends do not require credentials', () => {
  for (const backend of ['sqlite', 'mock', '']) assert.equal(Runtime.requiresCredentials({ backend }), false);
});
test('explicit server credential choice defers authorization to the server', () => {
  assert.equal(Runtime.requiresCredentials({ backend: 'oracle-thin', useServerCredentials: true }), false);
});

test('logon explains which credential is missing instead of repeating a generic prompt', () => {
  const missingPassword = Runtime.credentialError({ backend: 'oracle-thin', username: 'test_user', password: '' });
  assert.equal(missingPassword.field, 'password');
  assert.match(missingPassword.message, /password.*No connection was attempted/);
  const missingUser = Runtime.credentialError({ backend: 'oracle-thin', username: ' ', password: 'test' });
  assert.equal(missingUser.field, 'username');
  assert.equal(Runtime.credentialError({ backend: 'sqlite' }), null);
});

test('LOV rows are filtered by a case-insensitive prefix and keep their server index', () => {
  const lov = { rows: [['Acme Corp', 'Victoria'], ['Apex Ltd', 'Vancouver'], ['Birch Co', 'Kelowna']] };
  assert.equal(Runtime.lovRows(lov, '').length, 3);
  assert.deepEqual(Array.from(Runtime.lovRows(lov, 'ap').map(r => r.index)), [1]);
  assert.deepEqual(Array.from(Runtime.lovRows(lov, '  B').map(r => r.index)), [2]);
  assert.equal(Runtime.lovRows(lov, 'zzz').length, 0);
  assert.deepEqual(Array.from(Runtime.lovRows({ rows: [[null, 'x']] }, '').map(r => r.index)), [0]);
  assert.equal(Runtime.lovRows(undefined, 'a').length, 0);
});

test('timer polling is scheduled only while a timer is pending and nothing is waiting for an answer', () => {
  assert.equal(Runtime.timerPollDelay({ nextTimerMs: null }), null);
  assert.equal(Runtime.timerPollDelay({}), null);
  assert.equal(Runtime.timerPollDelay({ nextTimerMs: 0 }), 50, 'short and zero delays poll quickly');
  assert.equal(Runtime.timerPollDelay({ nextTimerMs: 1000 }), 1020);
  assert.equal(Runtime.timerPollDelay({ nextTimerMs: 600000 }), 5000, 'long timers are re-checked, never slept through');
  assert.equal(Runtime.timerPollDelay({ nextTimerMs: 10, lov: {} }), null);
  assert.equal(Runtime.timerPollDelay({ nextTimerMs: 10, alert: {} }), null);
});

test('form tabs: suspended callers and forms behind a modal called form are disabled', () => {
  const state = {
    activeForm: 2,
    forms: [
      { id: 1, name: 'MAIN', kind: 'root', suspended: true, modal: false },
      { id: 2, name: 'DETAIL', kind: 'called', suspended: false, modal: true },
    ],
  };
  const tabs = Array.from(Runtime.formTabs(state));
  assert.equal(tabs.length, 2);
  assert.equal(tabs[0].enabled, false);
  assert.equal(tabs[1].enabled, true);
  assert.equal(tabs[1].active, true);
  assert.match(tabs[1].label, /called/);
  const peers = Array.from(Runtime.formTabs({ activeForm: 1, forms: [{ id: 1, name: 'A', kind: 'root' }, { id: 2, name: 'B', kind: 'opened' }] }));
  assert.ok(peers.every(t => t.enabled), 'opened peers can be switched freely');
  assert.equal(Runtime.formTabs(undefined).length, 0);
});

test('events and answers are addressed to the form the browser shows', async () => {
  const sent = [];
  context.fetch = async (url, init) => { sent.push({ url, body: JSON.parse(init.body) }); return { ok: true, status: 200, json: async () => ({ sessionId: 's1', formId: 2 }) }; };
  const rt = new Runtime();
  rt.sessionId = 's1';
  rt.formId = 2;
  await rt.send({ kind: 'nextRecord' });
  await rt.answerAlert(1);
  await rt.answerLov(0);
  assert.deepEqual(sent.map(s => s.body.formId), [2, 2, 2]);
  assert.match(sent[1].url, /\/alert$/);
  assert.match(sent[2].url, /\/lov$/);
});

test('only http, https and same-origin URLs from form code are opened', () => {
  const ok = u => Runtime.safeActionUrl({ kind: 'openUrl', url: u });
  assert.equal(ok('https://example.org/x'), 'https://example.org/x');
  assert.equal(ok('http://example.org'), 'http://example.org');
  assert.equal(ok('/api/reports/x/html'), '/api/reports/x/html');
  assert.equal(ok('javascript:alert(1)'), null);
  assert.equal(ok('//evil.example/x'), null);
  assert.equal(ok('data:text/html,hi'), null);
  assert.equal(Runtime.safeActionUrl({ kind: 'other', url: 'https://example.org' }), null);
});
