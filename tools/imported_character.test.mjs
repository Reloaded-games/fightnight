import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {parseGLB} from '../web/imported/asset-loader.js';
import {parseUEAnim} from '../web/imported/ueanim.js';
import {packCharacter,CHARACTER_SLOTS} from '../web/imported/character-adapter.js';

const preset=JSON.parse(fs.readFileSync('dist/assets/imported/character-presets.json'));
const payload=e=>fs.readFileSync('assets/imported/'+decodeURIComponent(e.url));
const model=parseGLB(payload(preset.model));
const forward=preset.animations.find(e=>e.id===preset.states.find(s=>s.slot==='forward').id);
const clip=parseUEAnim(payload(forward));
const slots=['idle','forward','backward','left','right','crouch','jump','fall'];
const clips=slots.map(slot=>({slot,clip}));

test('real player bank contains all 25 native states and source weights, with no root-motion drift',()=>{
  const manifest=JSON.parse(fs.readFileSync('dist/assets/imported/character-runtime.json'));
  const bytes=fs.readFileSync('dist/assets/imported/character.fnchar');
  assert.equal(createHash('sha256').update(bytes).digest('hex'),manifest.sha256);
  assert.equal(bytes.length,manifest.bytes);
  assert.equal(bytes.toString('ascii',0,4),'FNC1');
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  const nodes=view.getUint32(4,true),palette=view.getUint32(8,true),count=view.getUint32(12,true),meshSize=view.getUint32(16,true),root=view.getUint32(32,true),vertices=view.getUint32(88,true);
  assert.equal(palette,88);assert.equal(count,25);assert.equal(vertices,model.vertexCount);
  let at=84+meshSize;
  const rootPose=bytes.subarray(at+root*44+4,at+root*44+44);
  at+=nodes*44+palette*68;
  for(let v=0;v<vertices;v++) {let total=0;for(let w=0;w<8;w++){assert.ok(view.getUint32(at+w*4,true)<palette);total+=view.getFloat32(at+32+w*4,true);}assert.ok(Math.abs(total-1)<.002);at+=64;}
  const states=new Set();
  for(let i=0;i<count;i++){
    const slot=view.getUint32(at,true),frames=view.getUint32(at+4,true);assert.ok(view.getFloat32(at+8,true)>0);at+=12;states.add(CHARACTER_SLOTS[slot]);
    for(let f=0;f<frames;f++){assert.deepEqual(bytes.subarray(at+root*40,at+root*40+40),rootPose);at+=nodes*40;}
  }
  assert.equal(at,bytes.length);assert.deepEqual([...states].sort(),[...CHARACTER_SLOTS].sort());
  assert.ok(manifest.states.some(s=>s.slot==='fall'&&s.resolution.adaptedReference));
});
test('packing cancels without mutating the original model or accepting incomplete/raw additive rigs',async()=>{
  const original=model.primitives[0].vertices.slice();
  await assert.rejects(packCharacter(model,clips,{isCurrent:()=>false}),/cancelada/);
  assert.deepEqual(model.primitives[0].vertices,original);
  await assert.rejects(packCharacter(model,clips.slice(1)),/incompleto/);
  await assert.rejects(packCharacter(model,[...clips,{slot:'land',clip:{...clip,additive:1}}]),/aditiva/);
  await assert.rejects(packCharacter({...model,rig:null},clips),/rig/);
});

test('a torso with a complete skeleton but no weighted head cannot replace the playable body',async()=>{
  const rig=model.rig,head=rig.nodes.findIndex(n=>n.name?.toLowerCase()==='head');
  const headNodes=new Set(rig.nodes.map((_,i)=>i).filter(i=>{for(let p=i;p>=0;p=rig.parents[p])if(p===head)return true;return false;}));
  const partial={...model,primitives:model.primitives.map(p=>({...p,sourceGeometry:{...p.sourceGeometry,skinGroups:p.sourceGeometry.skinGroups.map(g=>({...g,joints:g.joints.map(j=>headNodes.has(rig.skins[p.skinIndex].joints[j])?0:j)}))}}))};
  await assert.rejects(packCharacter(partial,clips),/Modelo parcial.*head/);
  await assert.rejects(packCharacter(model,clips.map(c=>({...c,clip:{...clip,tracks:new Map()}}))),/incompatível/);
});
