import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parseGLB } from '../web/imported/asset-loader.js';
import { bakeMesh, TARGET_LENGTH } from '../web/imported/mesh-adapter.js';

const model = parseGLB(readFileSync(new URL('../assets/cartoon/scout.glb', import.meta.url)));

function positions(bytes) {
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),result=[];
  for(let i=0;i<view.getUint32(4,true);i++)result.push([0,1,2].map(k=>view.getFloat32(12+i*32+k*4,true)));
  return result;
}

test('real GLB geometry becomes a grounded prop with unchanged triangle count',()=>{
  const bytes=bakeMesh(model,'prop'),pts=positions(bytes),view=new DataView(bytes.buffer);
  assert.equal(new TextDecoder().decode(bytes.subarray(0,4)),'FNM1');
  assert.equal(view.getUint32(8,true),model.triangleCount*3);
  assert.ok(Math.abs(Math.min(...pts.map(p=>p[1])))<1e-5);
  assert.ok(pts.every(p=>p.every(Number.isFinite)));
});

test('held models use the existing weapon scale and forward axis',()=>{
  for(const target of Object.keys(TARGET_LENGTH)) {
    const pts=positions(bakeMesh(model,target));
    const z=pts.map(p=>p[2]);
    assert.ok(Math.abs(Math.max(...z)-Math.min(...z)-TARGET_LENGTH[target])<1e-5);
  }
});

test('rejects placeholders, unknown slots and oversized meshes before allocation',()=>{
  assert.throws(()=>bakeMesh({...model,triangleCount:2}));
  assert.throws(()=>bakeMesh(model,'character'));
  assert.throws(()=>bakeMesh({...model,vertexCount:1000000}));
  assert.throws(()=>bakeMesh({...model,bounds:{...model.bounds,size:[0,0,0]}}));
});

test('rejects truncated or hostile GLB without GPU submission',()=>{
  assert.throws(()=>parseGLB(new Uint8Array(20)));
  const data=readFileSync(new URL('../assets/cartoon/scout.glb',import.meta.url));
  assert.throws(()=>parseGLB(data.subarray(0,data.length-4)));
});
