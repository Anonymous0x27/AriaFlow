// Run with: node browser-extension/tests/background.test.cjs
const assert = require('node:assert/strict');
const http = require('node:http');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');

(async () => {
  let requests = [], mode = 'ok';
  const server = http.createServer((req, res) => {
    let body = '';
    req.on('data', chunk => body += chunk);
    req.on('end', () => {
      requests.push({ url: req.url, headers: req.headers, body: JSON.parse(body) });
      res.writeHead(mode === 'ok' ? 201 : 401, { 'Content-Type': 'application/json' });
      res.end(mode === 'ok' ? '{"id":"test"}' : '{"error":"unauthorized"}');
    });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    for (const variant of ['chromium', 'firefox']) {
      let created, clicked, installed, cancels = [], erases = [], notices = [];
      let config = { apiUrl: `http://127.0.0.1:${server.address().port}`, token: 'secret', interceptAll: true };
      const chrome = {
        storage: { local: { get: async defaults => ({ ...defaults, ...config }) } },
        downloads: { onCreated: { addListener: fn => created = fn }, cancel: async id => cancels.push(id), erase: async arg => erases.push(arg) },
        notifications: { create: async notice => notices.push(notice) },
        runtime: { onInstalled: { addListener: fn => installed = fn } },
        contextMenus: { create: () => {}, onClicked: { addListener: fn => clicked = fn } },
      };
      const code = fs.readFileSync(path.join(__dirname, '..', variant, 'background.js'), 'utf8');
      vm.runInNewContext(code, { chrome, fetch, URL, AbortController, setTimeout, clearTimeout, console });
      requests = []; mode = 'ok'; installed();
      await created({ id: 11, url: 'https://example.com/file', filename: '/tmp/a.zip', referrer: 'https://example.com/page' });
      assert.equal(requests.length, 1);
      assert.equal(requests[0].url, '/api/v1/downloads');
      assert.equal(requests[0].headers.authorization, 'Bearer secret');
      assert.deepEqual(requests[0].body, { url: 'https://example.com/file', filename: 'a.zip', headers: [['Referer', 'https://example.com/page']] });
      assert.deepEqual(cancels, [11]); assert.equal(erases.length, 1); assert.equal(erases[0].id, 11);
      mode = 'error';
      await created({ id: 12, url: 'https://example.com/fail' });
      assert.deepEqual(cancels, [11]); assert.equal(notices.length, 1);
      await clicked({ menuItemId: 'ariaflow-download-link', linkUrl: 'https://example.com/link' }, { url: 'https://example.com/page' });
      assert.equal(notices.length, 2);
      mode = 'ok';
      await clicked({ menuItemId: 'ariaflow-download-link', linkUrl: 'https://example.com/link' }, { url: 'https://example.com/page' });
      assert.equal(requests.at(-1).body.url, 'https://example.com/link');
      const count = requests.length;
      await created({ id: 13, url: 'blob:https://example.com/id' });
      config.interceptAll = false;
      await created({ id: 14, url: 'https://example.com/file' });
      assert.equal(requests.length, count);
      config.interceptAll = true; config.apiUrl = 'http://example.com';
      await created({ id: 15, url: 'https://example.com/file' });
      assert.equal(requests.length, count); assert.deepEqual(cancels, [11]);
      console.log(`${variant}: success, auth failure, context menu, URL filtering and remote HTTP rejection passed`);
    }
  } finally { server.close(); }
})().catch(err => { console.error(err); process.exitCode = 1; });
