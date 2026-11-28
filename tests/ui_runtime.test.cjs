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
