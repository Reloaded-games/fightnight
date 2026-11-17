import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {parseUEAnim,sampleKeys} from '../web/imported/ueanim.js';
import {parseGLB} from '../web/imported/asset-loader.js';
import {bindAnimation,bakeAnimation} from '../web/imported/animation-player.js';

const catalog=JSON.parse(fs.readFileSync('dist/assets/imported/library.json'));
const animations=JSON.parse(fs.readFileSync('dist/assets/imported/types/animation.json'));
const payload=e=>fs.readFileSync('assets/imported/'+decodeURIComponent(e.url));
const modelEntry=catalog.assets.find(e=>e.label==='SK_M_MALE_Base');
const clipEntry=animations.find(e=>e.label==='DualWield_Idle_CMM');
const bytes=payload(clipEntry),clip=parseUEAnim(bytes);

test('native sequence moves real extracted vertices and restores the original bind pose',()=>{
  const model=parseGLB(payload(modelEntry)),first=model.primitives[0].vertices.slice(),player=bindAnimation(model,clip);
  assert.ok(player.matchedBones>=80);assert.equal(clip.additive,0);
  player.sample(0);const start=model.primitives[0].vertices.slice();player.sample(clip.duration*.4);
  assert.ok(start.some((v,i)=>Math.abs(v-model.primitives[0].vertices[i])>.02));
  for(const p of model.primitives)assert.ok(p.vertices.every(Number.isFinite));
  player.restore();assert.deepEqual(model.primitives[0].vertices,first);
});
test('linear position and shortest-path rotation interpolation are continuous',()=>{
  assert.deepEqual(sampleKeys([{frame:0,value:[0,0,0]},{frame:10,value:[10,20,30]}],5),[5,10,15]);
  assert.deepEqual(sampleKeys([],1,[3,4,5]),[3,4,5]);
  const q=sampleKeys([{frame:0,value:[0,0,0,1]},{frame:10,value:[0,0,0,-1]}],5,null,true);
  assert.deepEqual(q,[0,0,0,1]);
});
test('native parser refuses truncation, unsupported version and unparsed trailing bytes',()=>{
  for(const n of [0,8,20,bytes.length-1])assert.throws(()=>parseUEAnim(bytes.subarray(0,n)));
  const bad=Buffer.from(bytes);bad[18]=255;assert.throws(()=>parseUEAnim(bad),/versão/);
  assert.throws(()=>parseUEAnim(Buffer.concat([bytes,Buffer.from([0])])),/não interpretados/);
});
test('mismatched rigs and additive sequences cannot be silently applied',()=>{
  const model=parseGLB(payload(modelEntry));
  assert.throws(()=>bindAnimation(model,{...clip,additive:1}),/aditiva/);
  assert.throws(()=>bindAnimation(model,{...clip,tracks:new Map()}),/incompatíveis/);
  assert.throws(()=>bindAnimation({...model,rig:null},clip),/esqueleto/);
});
test('baked native clip respects frame and memory limits with actual differing frames',async()=>{
  const model=parseGLB(payload(modelEntry)),player=bindAnimation(model,clip),packed=await bakeAnimation(model,player);
  const view=new DataView(packed.buffer),count=view.getUint32(4,true),fps=view.getFloat32(8,true);
  assert.ok(count>=2&&count<=120);assert.ok(packed.length<64*1024*1024);
  assert.ok(Math.abs(count/fps-clip.duration)<1e-5);
  let offset=12;const frames=[];
  for(let i=0;i<count;i++){const length=view.getUint32(offset,true);frames.push(packed.slice(offset+4,offset+4+length));offset+=4+length;}
  assert.equal(offset,packed.length);assert.notDeepEqual(frames[0],frames[Math.floor(count/2)]);
});
test('cancelled baking restores the mesh and never returns a partial clip',async()=>{
  const model=parseGLB(payload(modelEntry)),first=model.primitives[0].vertices.slice(),player=bindAnimation(model,clip);let calls=0;
  await assert.rejects(()=>bakeAnimation(model,player,()=>++calls<3),/cancelada/);
  assert.deepEqual(model.primitives[0].vertices,first);
});
