import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {parseUEAnim,sampleKeys} from '../web/imported/ueanim.js';
import {parseGLB,nodeMatrix} from '../web/imported/asset-loader.js';
import {multiply} from '../web/imported/math.js';
import {composeAdditiveClip,indexBoneTracks,additiveSkeleton} from '../web/imported/additive-pose.js';
import {createAdditiveResolver,exportedRepresentation} from '../web/imported/additive-resolver.js';

const entries=JSON.parse(fs.readFileSync('dist/assets/imported/types/animation.json'));
const registry=JSON.parse(fs.readFileSync('docs/imported/additive-registry.json'));
const entry=name=>entries.find(e=>e.label===name);
const read=e=>{const b=fs.readFileSync('assets/imported/'+decodeURIComponent(e.url));assert.equal(createHash('sha256').update(b).digest('hex'),e.sha256);return b;};
const loadClip=async e=>parseUEAnim(read(e));
const near=(actual,expected,epsilon=1e-5)=>{assert.equal(actual.length,expected.length);actual.forEach((v,i)=>assert.ok(Math.abs(v-expected[i])<epsilon,`${actual} != ${expected}`));};
const q=(axis,angle)=>axis.map(v=>v*Math.sin(angle/2)).concat(Math.cos(angle/2));
const track=(position=[0,0,0],rotation=[0,0,0,1],scale=[1,1,1])=>({position:[{frame:0,value:position}],rotation:[{frame:0,value:rotation}],scale:[{frame:0,value:scale}]});
function clip(tracks,additive=0){return {name:'fixture',objectPath:'/Fixture/fixture.fixture',frames:2,fps:2,duration:1,additive,basePose:3,referenceFrame:0,tracks:new Map(Object.entries(tracks))};}

test('local additive translation, rotation order and scale respect weight zero and one',()=>{
  const base=clip({Root:track([5,7,9],q([0,0,1],Math.PI/2),[2,3,4])});
  const delta=clip({root:track([1,2,3],q([1,0,0],Math.PI/2),[.5,-.5,1])},1);
  const zero=composeAdditiveClip(delta,base,{weight:0}).tracks.get('Root');
  near(zero.position[0].value,[5,7,9]);near(zero.scale[0].value,[2,3,4]);near(zero.rotation[0].value,base.tracks.get('Root').rotation[0].value);
  const full=composeAdditiveClip(delta,base).tracks.get('Root');
  near(full.position[0].value,[6,9,12]);near(full.scale[0].value,[3,1.5,8]);
  near([...nodeMatrix({rotation:full.rotation[0].value})].slice(0,3),[0,0,1]);
  assert.equal(delta.additive,1);near(delta.tracks.get('root').scale[0].value,[.5,-.5,1]);
});

test('mesh-space offset preserves a child component orientation when its parent changes',()=>{
  const base=clip({Root:track([0,0,0],q([0,0,1],Math.PI/2)),child:track()});
  const delta=clip({root:track([0,0,0],q([1,0,0],Math.PI/2),[0,0,0]),child:track([0,0,0],[0,0,0,1],[0,0,0])},2);
  const resolved=composeAdditiveClip(delta,base,{skeleton:[{name:'root',parent:null},{name:'child',parent:'root'}]});
  const root=nodeMatrix({rotation:resolved.tracks.get('root').rotation[0].value}),child=nodeMatrix({rotation:resolved.tracks.get('child').rotation[0].value});
  near([...root].slice(0,3),[0,0,1]);near([...multiply(root,child)].slice(0,3),[0,1,0]);
});

test('invalid hierarchy, singular scales, ambiguous casing and out-of-range base frames fail',()=>{
  const base=clip({root:track(),child:track()}),delta=clip({root:track([0,0,0],[0,0,0,1],[0,0,0])},2);
  assert.throws(()=>composeAdditiveClip(delta,base),/hierarquia/);
  assert.throws(()=>composeAdditiveClip(delta,base,{skeleton:[{name:'root',parent:'child'},{name:'child',parent:'root'}]}),/Ciclo/);
  assert.throws(()=>composeAdditiveClip({...delta,additive:1},base,{referenceFrame:20}),/intervalo/);
  assert.throws(()=>composeAdditiveClip({...delta,additive:1},base,{weight:NaN}),/Peso/);
  assert.throws(()=>composeAdditiveClip(clip({root:track([0,0,0],[0,0,0,1],[-1,0,0])},1),base),/singular/);
  assert.throws(()=>composeAdditiveClip(clip({missing:track()},1),base),/sem o osso missing/);
  assert.throws(()=>indexBoneTracks(new Map([['Root',track()],['root',track()]])),/ambíguo/);
});

test('real GhostRock deltas are composed once with the explicitly adapted Core base',async()=>{
  const sourceEntry=entry('GhostRock_Jog_N_CMM'),source=await loadClip(sourceEntry),base=await loadClip(entry('GhostRock_Jog_N_CMM_Core'));
  assert.equal(exportedRepresentation(source,registry.exporter),'delta');
  near(source.tracks.get('pelvis').scale[0].value,[0,0,0]);
  const resolver=createAdditiveResolver({entries,loadClip,registry}),result=await resolver.resolve(sourceEntry);
  assert.equal(result.additive,0);assert.equal(result.resolution.adaptedReference,true);assert.equal(result.resolution.evidence.originalReferenceRecovered,false);
  const original=indexBoneTracks(base.tracks),composed=indexBoneTracks(result.tracks);
  near(composed.get('pelvis').track.position[0].value,original.get('pelvis').track.position[0].value);
  near(composed.get('pelvis').track.scale[0].value,[1,1,1]);
  assert.ok(composed.get('spine_01').track.scale[0].value.some((v,i)=>Math.abs(v-original.get('spine_01').track.scale[0].value[i])>.0001));
  for(const value of result.tracks.values())for(const keys of Object.values(value))for(const key of keys)assert.ok(key.value.every(Number.isFinite));
});

test('different source sample rates use duration-scaled sampling of the base',async()=>{
  const source=await loadClip(entry('GhostRock_Jog_S_CMM')),base=await loadClip(entry('GhostRock_Jog_S_CMM_Core'));
  assert.equal(source.frames,41);assert.equal(base.frames,121);assert.equal(source.duration,base.duration);
  const result=composeAdditiveClip(source,base,{weight:0});
  near(result.tracks.get('pelvis').position[20].value,sampleKeys(base.tracks.get('pelvis').position,20/41*121));
});

test('exporter provenance distinguishes already-expanded flags without applying a second base',async()=>{
  const actual=await loadClip(entry('GhostRock_Jog_N_CMM'));
  assert.equal(exportedRepresentation({...actual,basePose:1},registry.exporter),'absolute');
  assert.equal(exportedRepresentation({...actual,basePose:4},registry.exporter),'absolute');
  assert.throws(()=>exportedRepresentation(actual,{...registry.exporter,sourceSha256:{}}),/Procedência/);
  const resolver=createAdditiveResolver({entries,loadClip,registry});
  const expandedEntry=entry('AS_BeanImpact_Additive_Neutral'),expanded=await resolver.read(expandedEntry);
  assert.equal(expanded.additive,1);assert.equal(expanded.basePose,1);
  const result=await resolver.resolve(expandedEntry);
  assert.equal(result.tracks,expanded.tracks);assert.equal(result.resolution.kind,'already-expanded');
  near(result.tracks.get('pelvis').scale[0].value,[1,1,1]);
  await assert.rejects(()=>resolver.resolve(expandedEntry,{weight:0}),/deltas originais/);
  await assert.rejects(()=>resolver.resolve(entry('AM_Chicken_Bump_Additive')),/reconversão/);
});
test('registered idle, jump, fall and landing use explicit full reference frame zero',async()=>{
  const resolver=createAdditiveResolver({entries,loadClip,registry});
  for(const label of ['AssaultRifle_Idle_Patrol','JumpUp_NoWep_CMM_Additive','JumpFall_NoWep_CMM_Additive','JumpLand_NoWep_CMM_Additive']){
    const result=await resolver.resolve(entry(label));
    assert.equal(result.additive,0);assert.equal(result.resolution.baseMode,3);assert.equal(result.resolution.referenceFrame,0);
    assert.equal(result.resolution.adaptedReference,true);assert.equal(result.resolution.evidence.originalReferenceRecovered,false);
    for(const track of result.tracks.values())assert.ok(track.scale.every(k=>k.value.every(n=>Number.isFinite(n)&&Math.abs(n)>1e-8)));
  }
});

test('resolver rejects missing bases, cycles and wrong binding hashes',async()=>{
  const sourceEntry=entry('GhostRock_Jog_N_CMM'),emptyRegistry={...registry,bindings:[]};
  const resolver=createAdditiveResolver({entries,loadClip,registry:emptyRegistry});
  await assert.rejects(()=>resolver.resolve(sourceEntry),/sem pose-base/);
  await assert.rejects(()=>resolver.resolve(sourceEntry,{baseEntry:sourceEntry}),/Ciclo/);
  const binding={...registry.bindings.find(b=>b.sourceId===sourceEntry.id),baseSha256:'0'.repeat(64)};
  await assert.rejects(()=>createAdditiveResolver({entries,loadClip,registry:{...registry,bindings:[binding]}}).resolve(sourceEntry),/manifesto/);
  const explicit=await createAdditiveResolver({entries,loadClip,registry}).resolve(sourceEntry,{baseEntry:entry('GhostRock_Jog_S_CMM_Core')});
  assert.equal(explicit.resolution.kind,'user-selected-base');assert.equal(explicit.resolution.evidence,null);
});

test('real mesh-space aim offset resolves over an explicit compatible full pose',async()=>{
  const meshes=JSON.parse(fs.readFileSync('dist/assets/imported/library.json')).assets;
  const model=parseGLB(read(meshes.find(e=>e.label==='SK_M_MALE_Base')));
  const sourceEntry=entry('AO_AshtonMilo_Targeted_CD'),baseEntry=entry('AR_Targeted_Pose_CMM');
  const source=await loadClip(sourceEntry);assert.equal(source.additive,2);
  assert.equal(additiveSkeleton(model).length,88);
  const resolver=createAdditiveResolver({entries,loadClip,registry});
  const result=await resolver.resolve(sourceEntry,{baseEntry,model});
  assert.equal(result.additive,0);assert.equal(result.tracks.size,88);assert.equal(result.resolution.kind,'user-selected-base');
  assert.equal(result.resolution.adaptedReference,true);
  assert.ok([...result.tracks.values()].every(t=>t.scale.every(k=>k.value.every(n=>n>.5&&n<1.5))));
});
