const { chromium } = require('../apps/desktop/node_modules/playwright');
const http = require('node:http'), fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
// Build first: cd apps/desktop && npm ci && npm run build
// Run: node tests/completed-ui.cjs
const dist = path.resolve(__dirname, '../apps/desktop/dist');
const file = Buffer.from('AriaFlow completed file bytes\n');
const task = {id:'123',url:'https://example.org/a',filename:'fixture.txt',savePath:'/tmp/fixture.txt',totalSize:file.length,downloadedSize:file.length,status:'completed',speedBytesPerSec:0,progress:100,category:'other',createdAt:'2025-01-01T00:00:00Z'};
const history = {id:7,url:task.url,filename:task.filename,savePath:task.savePath,totalSize:file.length,averageSpeed:0,result:'completed',completedAt:'2025-01-01T00:00:00Z'};
let saves=[], requests=[];
function json(res, x) { res.writeHead(200, {'content-type':'application/json'}); res.end(JSON.stringify(x)); }
const server=http.createServer((req,res)=>{
 const url=new URL(req.url,'http://localhost'); requests.push({url:url.pathname,auth:req.headers.authorization});
 if(url.pathname==='/api/v1/status') return json(res,{version:'test',counts:{}});
 if(url.pathname==='/api/v1/settings') return json(res,{downloadDir:'/tmp',userAgents:[]});
 if(url.pathname==='/api/v1/downloads') return json(res,[task]);
 if(url.pathname==='/api/v1/history') return json(res,[history]);
 if(url.pathname.endsWith('/save-as')){let b='';req.on('data',x=>b+=x); return req.on('end',()=>{saves.push({url:url.pathname,body:JSON.parse(b)});json(res,{savedTo:'/tmp/copy.txt',size:file.length});});}
 if(url.pathname.endsWith('/file')) {res.writeHead(200,{'content-type':'application/octet-stream','content-disposition':'attachment; filename="fixture.txt"'});return res.end(file);}
 const filename=path.join(dist,url.pathname==='/'?'index.html':url.pathname); if(!filename.startsWith(dist) || !fs.existsSync(filename)){res.writeHead(404);return res.end()};
 res.writeHead(200,{'content-type':filename.endsWith('.html')?'text/html':filename.endsWith('.js')?'text/javascript':filename.endsWith('.css')?'text/css':'application/octet-stream'});fs.createReadStream(filename).pipe(res);
});
(async()=>{await new Promise(r=>server.listen(0,'127.0.0.1',r));const base=`http://127.0.0.1:${server.address().port}`;let browser;
try {browser=await chromium.launch({headless:true});
 const context=await browser.newContext({viewport:{width:1280,height:800},acceptDownloads:true,locale:'zh-CN'});
 const page=await context.newPage(); const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(base+'/#/downloads/completed'); const row=page.locator('.task-row');await row.waitFor();
 await row.click({button:'right'});assert.equal(await page.getByRole('menuitem',{name:'保存到浏览器/本地下载'}).count(),1);await page.getByRole('menuitem',{name:'另存为…'}).click();
 const overlay=page.locator('.dialog-backdrop');const box=await overlay.boundingBox();assert.deepEqual({x:box.x,y:box.y,width:box.width,height:box.height},{x:0,y:0,width:1280,height:800});
 assert.equal(await overlay.evaluate(el=>el.parentElement.tagName),'BODY');
 const [d1]=await Promise.all([page.waitForEvent('download'),page.getByRole('button',{name:'下载到当前设备'}).click()]);assert.equal(fs.readFileSync(await d1.path()).toString(),file.toString());
 const [d2]=await Promise.all([page.waitForEvent('download'),row.getByTitle('保存到浏览器/本地下载').click()]);assert.equal(fs.readFileSync(await d2.path()).toString(),file.toString());
 await page.evaluate(()=>localStorage.setItem('ariaflow.apiToken','test-token'));
 const [d3]=await Promise.all([page.waitForEvent('download'),row.getByTitle('保存到浏览器/本地下载').click()]);assert.equal(fs.readFileSync(await d3.path()).toString(),file.toString()); assert(requests.some(x=>x.url==='/api/v1/downloads/123/file'&&x.auth==='Bearer test-token'));
 await page.goto(base+'/#/history');const hrow=page.locator('tbody tr');await hrow.waitFor();
 const [d4]=await Promise.all([page.waitForEvent('download'),hrow.getByTitle('保存文件').click()]);assert.equal(fs.readFileSync(await d4.path()).toString(),file.toString());
 await hrow.getByTitle('另存为到指定目录…').click();assert.equal((await page.locator('.dialog-backdrop').boundingBox()).width,1280);await page.locator('#saveas-name').fill('copy.txt');await page.getByRole('button',{name:'保存',exact:true}).click();await page.waitForTimeout(250);assert(saves.some(x=>x.url==='/api/v1/history/7/save-as' && x.body.filename==='copy.txt'));
 assert.deepEqual(errors,[]);console.log('PASS web: right-click modal viewport, task/history browser downloads (token and no token), byte content and server copy');
 await context.close();
 const desktop=await browser.newContext({viewport:{width:1280,height:800},locale:'zh-CN'});
 await desktop.addInitScript(({task,history})=>{
 window.__requests=[]; window.__TAURI_INTERNALS__={invoke:async(cmd,args)=>{if(cmd!=='ariaflow_request') return null;window.__requests.push(args);if(args.path.startsWith('/api/v1/downloads?')||args.path==='/api/v1/downloads')return {status:200,body:[task]};if(args.path==='/api/v1/status')return {status:200,body:{version:'test',counts:{}}};if(args.path==='/api/v1/settings')return {status:200,body:{downloadDir:'/tmp',userAgents:[]}};if(args.path==='/api/v1/history?limit=100')return {status:200,body:[history]};if(args.path.endsWith('/save-as'))return {status:200,body:{savedTo:'/tmp/copy.txt',size:25}};return {status:200,body:[]};},transformCallback:()=>0};
 },{task,history});
 const p=await desktop.newPage();await p.goto(base+'/#/downloads/completed');await p.locator('.task-row').waitFor();assert.equal(await p.locator('.task-row').getByTitle('保存到浏览器/本地下载').count(), 0);await p.locator('.task-row').getByTitle('另存为到指定目录…').click();assert.equal((await p.locator('.dialog-backdrop').boundingBox()).width,1280);assert.equal(await p.getByRole('button',{name:'下载到当前设备'}).count(),0);
 await p.locator('#saveas-name').fill('copy.txt');await p.getByRole('button',{name:'保存',exact:true}).click();await p.waitForFunction(()=>window.__requests.some(x=>x.path==='/api/v1/downloads/123/save-as' && x.body.filename==='copy.txt'));await p.locator('.task-row').click({button:'right'}); assert.equal(await p.getByRole('menuitem',{name:'保存到浏览器/本地下载'}).count(),0); assert.equal(await p.getByRole('menuitem',{name:'另存为…'}).count(),1); await p.goto(base+'/#/history'); const historyRow=p.locator('tbody tr'); await historyRow.waitFor(); assert.equal(await historyRow.getByTitle('保存文件').count(),0); await historyRow.getByTitle('另存为到指定目录…').click(); assert.equal(await p.locator('.saveas-dialog').count(),1); console.log('PASS desktop-simulated IPC: save-as only, full viewport dialog and IPC save');await desktop.close();
}finally{if(browser)await browser.close();server.close();}})().catch(e=>{console.error(e);process.exitCode=1});
