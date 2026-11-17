// Exercise actual recovered geometry through the menu, WASM and WebGPU renderer.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {serve,launch} from './browser.mjs';

const {srv,url}=await serve();
const {browser,page,logs}=await launch({width:1280,height:720});
page.setDefaultTimeout(30000);
let checks=0;
const check=(name,condition)=>{assert.ok(condition,name);checks++;console.log('PASS '+name);};
try {
  await page.goto(url+'?nolock=1&quality=low&opts='+encodeURIComponent('bots=2;skipbus=1;god=1;map=island'));
  await page.waitForFunction(()=>window.__game?.state==='menu',null,{timeout:240000});
  const initial=await page.evaluate(()=>JSON.parse(window.__game.fn.imported_mesh_stats()).installed);
  check('the recovered rifle and shotgun load into the game by default',initial===2);
  await page.click('#btn-imported');
  await page.waitForSelector('.library-card',{timeout:240000});
  const count=await page.evaluate(()=>window.__game.importedLibrary.catalog.assets.length);
  check('complete model catalog loads in the Fightnight menu',count>0);
  const total=await page.evaluate(()=>window.__game.importedLibrary.catalog.summary.catalogAssets);
  check('all extracted content categories are indexed',total===678964);
  await page.fill('.library-filters input','SK_M_MALE_Base');
  await page.locator('.library-card').filter({hasText:'SK_M_MALE_Base'}).first().click();
  await page.waitForFunction(()=>!document.querySelector('[data-action="apply"]').disabled,null,{timeout:120000});
  check('real extracted model preview renders',await page.locator('.library-preview canvas').count()===1);
  await page.screenshot({path:'docs/imported/library-browser.png'});
  console.log('Applying recovered model from library');
  await page.click('[data-action="apply"]');
  try {
    await page.waitForFunction(()=>window.__game.state==='playing'&&!document.querySelector('.imported-library').open,null,{timeout:60000});
  } catch(error) {
    console.log(await page.evaluate(()=>({state:window.__game.state,feedback:document.querySelector('.library-feedback').textContent,loading:document.querySelector('#load-text').textContent})),logs);
    throw error;
  }
  const stats=()=>page.evaluate(()=>JSON.parse(window.__game.fn.imported_mesh_stats()));
  check('recovered model is submitted as a prop in the running match',(await stats()).props===1);
  await page.waitForTimeout(1200);
  check('GPU capture is available',await page.evaluate(()=>window.__game.freezeFrame()));
  await page.screenshot({path:'docs/imported/model-in-game.png'});
  await page.evaluate(()=>window.__game.unfreeze());
  await page.keyboard.press('Escape');
  await page.click('#btn-pause-imported');
  await page.selectOption('.library-target','rifle');
  // The existing selection remains valid across closing/reopening the library.
  await page.click('[data-action="apply"]');
  await page.waitForFunction(()=>!document.querySelector('.imported-library').open);
  check('same imported geometry can replace a held model without replacing gameplay',(await stats()).installed===initial+2);
  const before=await page.evaluate(()=>JSON.parse(window.__game.fn.debug('state')).pos);
  await page.keyboard.down('w');await page.waitForTimeout(900);await page.keyboard.up('w');
  const after=await page.evaluate(()=>JSON.parse(window.__game.fn.debug('state')).pos);
  check('player movement still works with imported geometry',Math.hypot(after[0]-before[0],after[2]-before[2])>.2);
  check('malformed runtime mesh is refused',await page.evaluate(()=>{try{window.__game.fn.install_imported_mesh('rifle',new Uint8Array(20));return false;}catch{return true;}}));
  await page.evaluate(()=>window.__game.fn.reset_imported_meshes());
  check('reset restores authored models and removes imported props',(await stats()).installed===0&&(await stats()).props===0);
  await page.keyboard.press('Escape');
  await page.click('#btn-pause-imported');
  await page.selectOption('.library-filters select','texture');
  await page.fill('.library-filters input','DefaultTexture');
  await page.waitForFunction(()=>document.querySelector('.library-card strong')?.textContent==='DefaultTexture',null,{timeout:240000});
  await page.locator('.library-card').first().click();
  await page.waitForFunction(()=>document.querySelector('.library-preview img')?.naturalWidth===128,null,{timeout:120000});
  check('an extracted texture passes SHA-256 and decodes in the browser',true);
  const textureURL=await page.locator('.library-download').getAttribute('href');
  await page.route(textureURL,route=>route.fulfill({status:200,body:'corrupted payload'}));
  await page.locator('.library-card').first().click();
  await page.waitForFunction(()=>document.querySelector('.library-details').textContent.includes('SHA-256 diferente'));
  check('corrupted texture bytes are refused before display',await page.locator('.library-preview img').count()===0);
  await page.unroute(textureURL);
  await page.click('[data-action="close"]');
  check('no JavaScript or WebGPU validation errors',!logs.some(s=>/\[pageerror\]|GPUValidationError|validation error/i.test(s)));
  fs.writeFileSync('docs/imported/browser-verification.json',JSON.stringify({modelEntries:count,totalEntries:total,checks,passed:true,logs},null,2));
} finally {await browser.close();srv.close();}
