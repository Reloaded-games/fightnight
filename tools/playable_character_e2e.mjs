// Actual keyboard/mouse play, WebGPU capture, rig state and error rollback.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {serve,launch} from './browser.mjs';

const {srv,url}=await serve(),{browser,page,logs}=await launch({width:1100,height:720});
page.setDefaultTimeout(90000);let checks=0;const evidence=[];
const check=(name,value)=>{assert.ok(value,name);checks++;console.log('PASS '+name);};
const dbg=c=>page.evaluate(c=>window.__game.fn.debug(c),c);
const stats=()=>page.evaluate(()=>JSON.parse(window.__game.fn.imported_character_stats()));
const state=async()=>JSON.parse(await dbg('state'));
const hud=()=>page.evaluate(()=>JSON.parse(window.__game.fn.hud()));
const waitState=name=>page.waitForFunction(name=>JSON.parse(window.__game.fn.imported_character_stats()).state===name,name);
const advance=async(seconds=.3)=>{const start=(await state()).t;await page.waitForFunction(t=>JSON.parse(window.__game.fn.debug('state')).t>=t,start+seconds);};
const capture=async(name)=>{await advance(.4);check('GPU readback '+name,await page.evaluate(()=>window.__game.freezeFrame()));await page.screenshot({path:`docs/imported/player-${name}.png`});await page.evaluate(()=>window.__game.unfreeze());evidence.push({name,...await stats()});};
try {
  await page.addInitScript(()=>localStorage.setItem('fightnight.settings',JSON.stringify({autosprint:false,skipbus:true})));
  await page.goto(url+'?nolock=1&autostart=1&quality=low&opts='+encodeURIComponent('skipbus=1;bots=2;god=1;map=island'));
  await page.waitForFunction(()=>window.__game?.state==='playing',null,{timeout:240000});
  await dbg('freeze_bots');await dbg('tp 52 28');await dbg('look 0 0');await advance(.6);
  const first=await stats();check('default player uses the native 88-bone skin and 25-state bank',first.active&&first.drawn&&first.bones===88&&first.clips===25&&first.vertices===6551);
  await capture('idle');
  await dbg('give ar rare');await page.mouse.down({button:'right'});await advance(.3);
  for(const [key,name] of [['KeyW','forward'],['KeyS','backward'],['KeyA','left'],['KeyD','right']]){
    const before=await state();await page.keyboard.down(key);await waitState(name);await advance(.4);const after=await state();
    check('native '+name+' follows real movement',Math.hypot(after.pos[0]-before.pos[0],after.pos[2]-before.pos[2])>.5);
    await page.keyboard.up(key);await advance(.3);
  }
  await page.mouse.up({button:'right'});await page.keyboard.down('ShiftLeft');await page.keyboard.down('KeyW');await waitState('sprint');check('Shift+W selects the native sprint',true);await capture('sprint');await page.keyboard.up('KeyW');await page.keyboard.up('ShiftLeft');await advance(.5);
  await page.keyboard.down('KeyF');await waitState('crouch');check('crouch enters its native pose',true);
  await page.mouse.down({button:'right'});await page.keyboard.down('KeyD');await waitState('crouch_right');check('crouch strafe uses its own native cycle',true);await page.keyboard.up('KeyD');await capture('crouch');check('crouching actually lowers the deformed body',(await stats()).height<1.4);await page.keyboard.up('KeyF');await page.mouse.up({button:'right'});await advance(.4);
  await page.keyboard.down('Space');await waitState('jump');await page.keyboard.up('Space');check('Space selects the native takeoff',true);await waitState('fall');check('falling selects the resolved additive fall pose',true);await waitState('land');check('ground contact selects the resolved additive landing',true);await advance(.6);
  await dbg('give ar rare');await waitState('armed_idle');await page.mouse.down({button:'right'});await advance(.4);
  const aimed=await stats();check('native arms follow the aiming weapon',aimed.aim>.9&&aimed.handError<.16);await capture('aim');
  const mag=(await hud()).slots[(await hud()).sel].a;await page.mouse.down();await advance(.4);await page.mouse.up();
  check('shooting still consumes the authoritative magazine',(await hud()).slots[(await hud()).sel].a<mag);await page.mouse.up({button:'right'});
  await page.keyboard.press('KeyR');await page.waitForFunction(()=>JSON.parse(window.__game.fn.imported_character_stats()).layer==='reload');check('reload overlays the native upper-body clip',true);await capture('reload');await advance(3);
  check('reload finishes with a refilled magazine',(await hud()).slots[(await hud()).sel].a===mag);
  await dbg('tree 0');await advance(.4);const wood=(await hud()).mats[0];await page.mouse.down();await page.waitForFunction(()=>JSON.parse(window.__game.fn.imported_character_stats()).layer==='swing');await advance(1.5);await page.mouse.up();
  check('native pickaxe swings still harvest resources',(await hud()).mats[0]>wood);
  await dbg('tp 52 28');await advance(.6);await page.keyboard.press('KeyB');await waitState('emote');check('B plays the native emote',true);await capture('emote');await page.keyboard.press('KeyB');await advance(.4);
  await dbg('sky 52 200 28');await waitState('dive');check('freefall plays the native dive',true);await capture('dive');check('diving actually rotates the deformed body horizontally',(await stats()).height<.9&&(await stats()).depth>1.3);await dbg('glide 52 80 28');await waitState('glide');check('glider plays its native hanging pose',true);await capture('glide');check('native hands reach the actual glider bar',(await stats()).handError<.05);
  await dbg('tp 52 28');await dbg('vehicle 0');await advance(.5);await page.keyboard.press('KeyE');await waitState('vehicle');check('entering a vehicle selects the driver clip',true);await capture('vehicle');await page.keyboard.press('KeyE');await advance(.5);
  await page.keyboard.press('Escape');const stopped=await stats();await page.waitForTimeout(400);check('pause freezes both the pose clock and GPU skin updates',(await stats()).updates===stopped.updates&&(await stats()).clock===stopped.clock);
  check('malformed character is rejected atomically',await page.evaluate(()=>{const f=window.__game.fn,before=f.imported_character_stats();try{f.install_imported_character(new Uint8Array(84));return false;}catch{return f.imported_character_stats()===before;}}));
  await page.click('#btn-quit');await page.click('#btn-play');await page.waitForFunction(()=>window.__game.state==='playing');await advance(.4);check('a new match retains the imported player',(await stats()).active&&(await stats()).drawn);
  await page.keyboard.press('Escape');await page.click('#btn-pause-imported');await page.getByText('Restaurar personagem do Fightnight',{exact:true}).click();check('restore player keeps imported weapons installed',!(await stats()).active&&await page.evaluate(()=>JSON.parse(window.__game.fn.imported_mesh_stats()).installed>0));
  await page.getByRole('button',{name:'Fechar biblioteca',exact:true}).click();await page.keyboard.press('Escape');await advance(.3);await capture('restored');
  check('no JavaScript or WebGPU validation errors',!logs.some(s=>/\[pageerror\]|GPUValidationError|validation error|Personagem importado:/i.test(s)));
  fs.writeFileSync('docs/imported/playable-character-browser-verification.json',JSON.stringify({checks,passed:true,evidence,logs},null,2));
}catch(error){console.error({state:await state().catch(()=>null),rig:await stats().catch(()=>null),logs});throw error;}
finally{await browser.close();srv.close();}
