import test from 'node:test';
import assert from 'node:assert/strict';
import {parseGLB,nodeMatrix,normalMatrix,fetchAssetBytes,loadGLB} from '../web/imported/asset-loader.js';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';

function fixture(edit=()=>{}) {
 const chunks=[],views=[];let size=0;
 function add(data,properties={}){size=Math.ceil(size/4)*4;const bytes=new Uint8Array(data.buffer,data.byteOffset,data.byteLength);views.push({buffer:0,byteOffset:size,byteLength:bytes.length,...properties});chunks.push({bytes,start:size});size+=bytes.length;return views.length-1;}
 const positions=add(new Float32Array([0,0,0,1,0,0,0,1,0]));
 const indices=add(new Uint16Array([0,1,2]));
 const json={asset:{version:'2.0'},buffers:[{byteLength:0}],bufferViews:views,accessors:[{bufferView:positions,componentType:5126,count:3,type:'VEC3'},{bufferView:indices,componentType:5123,count:3,type:'SCALAR'}],meshes:[{name:'triangle',primitives:[{attributes:{POSITION:0},indices:1}]}],nodes:[{mesh:0}],scenes:[{nodes:[0]}],scene:0};
 edit(json,add);json.buffers[0].byteLength=size;
 const bin=new Uint8Array(Math.ceil(size/4)*4);for(const part of chunks)bin.set(part.bytes,part.start);
 const encoded=new TextEncoder().encode(JSON.stringify(json)),jsonBytes=new Uint8Array(Math.ceil(encoded.length/4)*4).fill(32);jsonBytes.set(encoded);
 const result=new Uint8Array(12+8+jsonBytes.length+8+bin.length),header=new DataView(result.buffer);
 header.setUint32(0,0x46546c67,true);header.setUint32(4,2,true);header.setUint32(8,result.length,true);
 header.setUint32(12,jsonBytes.length,true);header.setUint32(16,0x4e4f534a,true);result.set(jsonBytes,20);
 const offset=20+jsonBytes.length;header.setUint32(offset,bin.length,true);header.setUint32(offset+4,0x004e4942,true);result.set(bin,offset+8);
 return result;
}
const close=(a,b)=>assert.ok(Math.abs(a-b)<1e-5,`${a} !== ${b}`);

test('GLB extracts actual indexed triangles, generates normals, and computes transformed scene bounds',()=>{
 const model=parseGLB(fixture(json=>{json.nodes=[{translation:[2,0,0],children:[1]},{mesh:0,translation:[0,4,0],scale:[2,3,1]}];}));
 assert.equal(model.vertexCount,3);assert.equal(model.triangleCount,1);assert.equal(model.meshCount,1);
 assert.deepEqual(model.bounds.min,[2,4,0]);assert.deepEqual(model.bounds.max,[4,7,0]);
 assert.deepEqual([...model.primitives[0].indices],[0,1,2]);
 assert.deepEqual([...model.primitives[0].vertices.subarray(3,6)],[0,0,1]);
 assert.equal(model.metadata.animationPlayback,false);
});
test('glTF quaternion transforms and inverse-transpose normals preserve nonuniform-scale lighting',()=>{
 const m=nodeMatrix({rotation:[0,Math.sin(Math.PI/4),0,Math.cos(Math.PI/4)],scale:[2,3,4]});
 close(m[0],0);close(m[2],-2);close(m[8],4);close(m[10],0);
 const n=normalMatrix(nodeMatrix({scale:[2,3,4]}));close(n[0],.5);close(n[4],1/3);close(n[8],.25);
 assert.throws(()=>normalMatrix(nodeMatrix({scale:[0,1,1]})),/singular/);
});
test('interleaved accessors preserve normalized vertex colors and UV coordinates',()=>{
 const model=parseGLB(fixture((json,add)=>{
  const stream=new Uint8Array(48),v=new DataView(stream.buffer);
  const positions=[[0,0,0],[1,0,0],[0,1,0]],colors=[[255,0,0,255],[0,255,0,255],[0,0,255,128]];
  for(let i=0;i<3;i++){for(let k=0;k<3;k++)v.setFloat32(i*16+k*4,positions[i][k],true);stream.set(colors[i],i*16+12);}
  const view=add(stream,{byteStride:16}),uv=add(new Uint16Array([0,0,65535,0,0,65535]));
  json.accessors[0].bufferView=view;
  json.accessors.push({bufferView:view,byteOffset:12,componentType:5121,normalized:true,count:3,type:'VEC4'},{bufferView:uv,componentType:5123,normalized:true,count:3,type:'VEC2'});
  json.meshes[0].primitives[0].attributes={POSITION:0,COLOR_0:2,TEXCOORD_0:3};
 }));
 const vertices=model.primitives[0].vertices;
 assert.deepEqual([...vertices.subarray(8,12)],[1,0,0,1]);
 close(vertices[2*12+11],128/255);close(vertices[12+6],1);close(vertices[24+7],1);
 assert.equal(model.sourceColorStats.allRGBZero,false);
});
test('sparse positions are applied and unordered sparse indices are rejected',()=>{
 const make=bad=>fixture((json,add)=>{
  const indices=add(new Uint8Array(bad?[0,0,2]:[0,1,2])),values=add(new Float32Array([0,0,0,2,0,0,0,3,0]));
  json.accessors[0]={componentType:5126,count:3,type:'VEC3',sparse:{count:3,indices:{bufferView:indices,componentType:5121},values:{bufferView:values}}};
 });
 assert.deepEqual(parseGLB(make(false)).bounds.max,[2,3,0]);
 assert.throws(()=>parseGLB(make(true)),/sparse.*ordem/);
});
test('triangle strips produce correct alternating winding',()=>{
 const model=parseGLB(fixture((json,add)=>{
  json.accessors[0]={bufferView:add(new Float32Array([0,0,0,1,0,0,0,1,0,1,1,0])),componentType:5126,count:4,type:'VEC3'};
  json.accessors[1]={bufferView:add(new Uint8Array([0,1,2,3])),componentType:5121,count:4,type:'SCALAR'};
  json.meshes[0].primitives[0].mode=5;
 }));
 assert.deepEqual([...model.primitives[0].indices],[0,1,2,2,1,3]);assert.equal(model.triangleCount,2);
});
test('skin joint weights and inverse-bind matrices render the preserved bind pose, while animation links remain metadata',()=>{
 const model=parseGLB(fixture((json,add)=>{
  const joints=add(new Uint8Array([0,0,0,0,0,0,0,0,0,0,0,0])),weights=add(new Uint8Array([255,0,0,0,255,0,0,0,255,0,0,0]));
  const bind=add(new Float32Array([1,0,0,0,0,1,0,0,0,0,1,0,-2,0,0,1]));
  json.accessors.push({bufferView:joints,componentType:5121,count:3,type:'VEC4'},{bufferView:weights,componentType:5121,normalized:true,count:3,type:'VEC4'},{bufferView:bind,componentType:5126,count:1,type:'MAT4'});
  json.meshes[0].primitives[0].attributes={POSITION:0,JOINTS_0:2,WEIGHTS_0:3};
  json.nodes=[{mesh:0,skin:0,translation:[10,0,0]},{name:'hip',translation:[5,0,0]}];json.scenes[0].nodes=[0,1];
  json.skins=[{name:'rig',joints:[1],inverseBindMatrices:4,skeleton:1}];
  json.animations=[{name:'idle-link',channels:[{sampler:0,target:{node:1,path:'translation'}}],samplers:[{input:5,output:6}]}];
 }));
 assert.deepEqual(model.bounds.min,[3,0,0]);assert.deepEqual(model.bounds.max,[4,1,0]);
 assert.equal(model.skinned,true);assert.equal(model.metadata.displayMode,'bind-pose');assert.equal(model.metadata.animationPlayback,false);
 assert.deepEqual(model.metadata.skins[0].joints,[1]);assert.equal(model.metadata.animations[0].channels[0].path,'translation');
});
test('embedded base-color PNG linkage, material factors and UV transforms are preserved',()=>{
 const model=parseGLB(fixture((json,add)=>{
  const image=add(new Uint8Array([137,80,78,71,13,10,26,10])),uv=add(new Float32Array([0,0,1,0,0,1]));
  json.images=[{bufferView:image,mimeType:'image/png'}];json.textures=[{source:0}];
  json.materials=[{name:'linked',pbrMetallicRoughness:{baseColorFactor:[.2,.3,.4,1],baseColorTexture:{index:0,extensions:{KHR_texture_transform:{offset:[.1,.2],scale:[2,3]}}}}}];
  json.accessors.push({bufferView:uv,componentType:5126,count:3,type:'VEC2'});
  json.meshes[0].primitives[0].material=0;json.meshes[0].primitives[0].attributes.TEXCOORD_0=2;
 }));
 assert.deepEqual(model.materials[0].baseColorFactor,[.2,.3,.4,1]);assert.equal(model.materials[0].baseColorTexture.image.bytes.length,8);
 close(model.primitives[0].vertices[12+6],2.1);close(model.primitives[0].vertices[24+7],3.2);
});
test('secondary joint and weight sets contribute to the bind pose',()=>{
 const model=parseGLB(fixture((json,add)=>{
  const j0=add(new Uint8Array(12)),j1=add(new Uint8Array([1,0,0,0,1,0,0,0,1,0,0,0]));
  const w=add(new Float32Array([.5,0,0,0,.5,0,0,0,.5,0,0,0]));
  json.accessors.push({bufferView:j0,componentType:5121,count:3,type:'VEC4'},{bufferView:j1,componentType:5121,count:3,type:'VEC4'},{bufferView:w,componentType:5126,count:3,type:'VEC4'});
  json.meshes[0].primitives[0].attributes={POSITION:0,JOINTS_0:2,JOINTS_1:3,WEIGHTS_0:4,WEIGHTS_1:4};
  json.nodes=[{mesh:0,skin:0,translation:[50,0,0]},{translation:[2,0,0]},{translation:[4,0,0]}];json.scenes[0].nodes=[0,1,2];json.skins=[{joints:[1,2]}];
 }));
 assert.deepEqual(model.bounds.min,[3,0,0]);assert.deepEqual(model.bounds.max,[4,1,0]);
});
test('malformed headers, offsets, indices, cycles and unsupported required extensions fail explicitly',()=>{
 const badLength=fixture();new DataView(badLength.buffer).setUint32(8,badLength.length+4,true);assert.throws(()=>parseGLB(badLength),/comprimento/);
 assert.throws(()=>parseGLB(fixture(json=>{json.accessors[0].byteOffset=4;})),/fora do bufferView/);
 assert.throws(()=>parseGLB(fixture((json,add)=>{json.accessors[1].bufferView=add(new Uint16Array([0,1,3]));})),/índice de vértice/);
 assert.throws(()=>parseGLB(fixture(json=>{json.nodes=[{mesh:0,children:[1]},{children:[0]}];})),/ciclo/);
 assert.throws(()=>parseGLB(fixture(json=>{json.extensionsRequired=['KHR_draco_mesh_compression'];})),/extensão obrigatória/);
 assert.throws(()=>parseGLB(fixture(),{maxVertices:2}),/limite de vértices/);
 assert.throws(()=>parseGLB(fixture(),{maxDecodedBytes:16}),/memória decodificada/);
});
test('asset fetch refuses cross-origin resources and enforces response sizes before decoding',async()=>{
 let calls=0;
 const fetchImpl=async()=>{calls++;return {ok:true,headers:new Headers({'content-length':'100'}),arrayBuffer:async()=>new ArrayBuffer(100)};};
 await assert.rejects(fetchAssetBytes('https://other.example/model.glb',{baseURL:'http://127.0.0.1:4175/',fetchImpl}),/mesma origem/);assert.equal(calls,0);
 await assert.rejects(fetchAssetBytes('/model.glb',{baseURL:'http://127.0.0.1:4175/',fetchImpl,maxBytes:20}),/limite de memória/);
});
test('loadGLB verifies the imported manifest SHA-256 and attaches provenance',async()=>{
 const bytes=fixture(),hash=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),n=>n.toString(16).padStart(2,'0')).join('');
 const fetchImpl=async()=>new Response(bytes,{headers:{'content-type':'model/gltf-binary'}});
 const entry={id:'real-identity',url:'/mesh.glb',contentHash:hash,provenance:{className:'StaticMesh'}};
 const model=await loadGLB(entry,{baseURL:'http://127.0.0.1:4175/',fetchImpl});
 assert.equal(model.entry.id,'real-identity');assert.equal(model.triangleCount,1);
 await assert.rejects(loadGLB({...entry,contentHash:'0'.repeat(64)},{baseURL:'http://127.0.0.1:4175/',fetchImpl}),/SHA-256/);
});
test('the actually extracted engine Cube preserves 54 vertices and 48 triangles',async t=>{
 const path=new URL('../assets/imported/files/38f43fdfa4dce5b8b3163b1aa95d88551e07303198016d3b3beeca28b6c6dacd.glb',import.meta.url);
 let bytes;try{bytes=await readFile(path);}catch(error){if(error.code==='ENOENT'){t.skip('Verified local extraction sample is not included in this checkout.');return;}throw error;}
 assert.equal(createHash('sha256').update(bytes).digest('hex'),'38f43fdfa4dce5b8b3163b1aa95d88551e07303198016d3b3beeca28b6c6dacd');
 const model=parseGLB(bytes);assert.equal(model.vertexCount,54);assert.equal(model.triangleCount,48);assert.deepEqual(model.bounds.min,[-.5,-.5,-.5]);assert.deepEqual(model.bounds.max,[.5,.5,.5]);
 assert.equal(model.metadata.displayMode,'static');assert.equal(model.skinned,false);
 assert.deepEqual(model.sourceColorStats,{present:true,vertexCount:54,zeroRGBA:54,blackRGB:54,allRGBZero:true});
 for(let i=8;i<model.primitives[0].vertices.length;i+=12)assert.deepEqual([...model.primitives[0].vertices.subarray(i,i+4)],[0,0,0,0]);
 assert.deepEqual(model.primitives[0].material.baseColorFactor,[1,1,1,1]);
});
