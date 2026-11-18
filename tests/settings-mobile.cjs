// After building apps/desktop, run: node tests/settings-mobile.cjs
const { chromium } = require('../apps/desktop/node_modules/playwright');
const http = require('node:http'), fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const dist = path.resolve(__dirname, '../apps/desktop/dist');
const longUA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36';
let settings = {downloadDir:'/home/test/very-long-directory-name/Downloads',maxConcurrentTasks:5,maxConnectionsPerTask:16,maxTotalConnections:64,retryCount:5,globalSpeedLimit:0,perTaskSpeedLimit:0,userAgent:longUA,userAgents:[longUA,longUA+' Edg/157.0.0.0','Wget2/2.3'],apiBind:'127.0.0.1:8787'};
const json=(res,data)=>{res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(data))};
const server=http.createServer((req,res)=>{
 const pathname=new URL(req.url,'http://localhost').pathname;
 if(pathname==='/api/v1/settings') { if(req.method==='PUT'){let b='';req.on('data',c=>b+=c);return req.on('end',()=>{settings={...settings,...JSON.parse(b)};json(res,settings)});}return json(res,settings); }
 if(pathname==='/api/v1/status')return json(res,{version:'test',counts:{}});
 if(pathname==='/api/v1/downloads'||pathname==='/api/v1/proxies'||pathname==='/api/v1/certificates')return json(res,[]);
 if(pathname.startsWith('/api/'))return json(res,{});
 const f=path.join(dist,pathname==='/'?'index.html':pathname);
 if(!f.startsWith(dist)||!fs.existsSync(f)){res.writeHead(404);return res.end();}
 res.writeHead(200,{'content-type':f.endsWith('.html')?'text/html':f.endsWith('.js')?'text/javascript':f.endsWith('.css')?'text/css':'application/octet-stream'});fs.createReadStream(f).pipe(res);
});
(async()=>{await new Promise(r=>server.listen(0,'127.0.0.1',r));const base=`http://127.0.0.1:${server.address().port}`;let browser;
 try {browser=await chromium.launch({headless:true});
 for(const width of [320,360,390,768,1024]){
  const context=await browser.newContext({viewport:{width,height:800},locale:'zh-CN'});const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto(base+'/#/settings');const card=page.locator('section.card').filter({has:page.getByRole('heading',{name:'下载设置'})});await card.locator('.ua-item').first().waitFor();
  const cardBounds=await card.boundingBox(); assert(cardBounds.x >= 0 && cardBounds.x + cardBounds.width <= width + 0.5, `card extends beyond ${width}px viewport`);
  const overflow=await card.evaluate(el=>{const c=el.getBoundingClientRect();return [...el.querySelectorAll('.af-input,.save-dir-row,.ua-manager,.ua-item,.ua-item button,.ua-add button')].map(n=>({name:n.className,r:n.getBoundingClientRect()})).filter(x=>x.r.width && (x.r.left<c.left-0.5 || x.r.right>c.right+0.5)).map(x=>({name:x.name,left:x.r.left,right:x.r.right,cardLeft:c.left,cardRight:c.right}));});
  assert.deepEqual(overflow,[],`${width}px download settings overflow: ${JSON.stringify(overflow)}`);
  await card.locator('.ua-add input').fill('TestAgent/1.0');await card.locator('.ua-add button').click();assert(await card.locator('.ua-text').getByText('TestAgent/1.0').count());
  await card.getByRole('button',{name:'保存下载设置'}).click();await page.waitForFunction(()=>document.body.innerText.includes('已保存到 ariaflowd'));assert(settings.userAgents.includes('TestAgent/1.0'));
  const proxy=page.locator('section.card').filter({has:page.getByRole('heading',{name:'命名代理'})});await proxy.getByRole('button',{name:'添加代理'}).click();const select=proxy.locator('.asel-trigger');await select.click();const geom=await proxy.evaluate(el=>{const c=el.getBoundingClientRect();const a=el.querySelector('.asel-trigger').getBoundingClientRect(),b=el.querySelector('.asel-pop').getBoundingClientRect();return {left:c.left,right:c.right,triggerLeft:a.left,triggerRight:a.right,popLeft:b.left,popRight:b.right}});assert(geom.triggerLeft>=geom.left && geom.triggerRight<=geom.right+0.5 && geom.popLeft>=geom.left && geom.popRight<=geom.right+0.5,JSON.stringify(geom));
  await proxy.getByRole('option',{name:'socks5',exact:true}).click();assert((await select.textContent()).includes('socks5'));assert.deepEqual(errors,[]);await context.close();settings.userAgents=settings.userAgents.filter(x=>x!=='TestAgent/1.0');console.log(`PASS ${width}px: fields, UA management and themed select fit; save/select work`);
 }
 }finally{if(browser)await browser.close();server.close();}
})().catch(e=>{console.error(e);process.exitCode=1});
