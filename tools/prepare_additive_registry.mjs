// Record explicit Fightnight adaptation choices with source hashes and skeleton compatibility.
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {parseUEAnim} from '../web/imported/ueanim.js';
import {exportedRepresentation} from '../web/imported/additive-resolver.js';

const root=fileURLToPath(new URL('..',import.meta.url));
const catalog=JSON.parse(fs.readFileSync(path.join(root,'dist/assets/imported/types/animation.json'),'utf8'));
const exporter=JSON.parse(fs.readFileSync(path.join(root,'docs/imported/additive-exporter-proof.json'),'utf8').replace(/^\uFEFF/,''));
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
function load(label) {
  const candidates=catalog.filter(entry=>entry.label===label),unique=new Map(candidates.map(e=>[e.sha256,e]));
  if(unique.size!==1)throw new Error(`Missing or ambiguous animation: ${label}`);
  const entry=[...unique.values()][0],bytes=fs.readFileSync(path.join(root,'assets/imported',decodeURIComponent(entry.url)));
  if(sha(bytes)!==entry.sha256)throw new Error(`Source hash changed: ${label}`);
  const match=decodeURIComponent(entry.url).match(/^\.\/sources\/main\/([^/]+)\/(.*?\.uasset)\//);
  if(!match)throw new Error('Expected a primary archived export');
  const metadataPath='docs/imported/fortnite-analysis/assets/metadata/metadata/'+match[1]+'/'+match[2]+'.json';
  const metadataBytes=fs.readFileSync(path.join(root,metadataPath)),document=JSON.parse(metadataBytes);
  const exported=document.exports?.find(e=>e.Type==='AnimSequence'&&e.Name===label);
  if(!document.complete||!exported)throw new Error(`Incomplete source metadata: ${label}`);
  return {entry,clip:parseUEAnim(bytes),metadata:exported,metadataSha256:sha(metadataBytes),metadataPath};
}
const bindings=[];
const pairs=[];
for(const prefix of ['GhostRock_Jog','ExplosiveBow_Jog','ExplosiveBow_Relaxed_Jog'])for(const direction of ['N','S','E','W'])
  pairs.push({sourceLabel:`${prefix}_${direction}_CMM`,baseLabel:`${prefix}_${direction}_CMM_Core`,baseMode:2});
for(const [sourceLabel,baseLabel] of [
  ['AssaultRifle_Idle_Patrol','AssaultRifle_Idle_Patrol_Pose'],
  ['JumpUp_NoWep_CMM_Additive','NoWep_Jump_From_Idle'],
  ['JumpFall_NoWep_CMM_Additive','NoWep_Jump_From_Idle'],
  ['JumpLand_NoWep_CMM_Additive','NoWep_Jump_From_Idle'],
])pairs.push({sourceLabel,baseLabel,baseMode:3});
for(const pair of pairs){
  const source=load(pair.sourceLabel),base=load(pair.baseLabel);
  if(exportedRepresentation(source.clip,exporter)!=='delta'||base.clip.additive!==0)throw new Error('Expected raw delta and complete base');
  const s=source.metadata,b=base.metadata;
  if(!s.SkeletonGuid||s.SkeletonGuid!==b.SkeletonGuid||s.Properties.RetargetSource!==b.Properties.RetargetSource||
      (pair.baseMode===2&&Math.abs(source.clip.duration-base.clip.duration)>.001))
    throw new Error(JSON.stringify({error:'Adapted base does not have matching source skeleton, retarget or timing',source:source.entry.label,base:base.entry.label,skeleton:[s.SkeletonGuid,b.SkeletonGuid],retarget:[s.Properties.RetargetSource,b.Properties.RetargetSource],frames:[source.clip.frames,base.clip.frames],duration:[source.clip.duration,base.clip.duration]}));
  bindings.push({sourceId:source.entry.id,sourceLabel:source.entry.label,sourceSha256:source.entry.sha256,
    baseId:base.entry.id,baseLabel:base.entry.label,baseSha256:base.entry.sha256,
    adaptedReference:true,baseMode:pair.baseMode,referenceFrame:0,
    evidence:{skeletonGuid:s.SkeletonGuid,retargetSource:s.Properties.RetargetSource,frames:source.clip.frames,baseFrames:base.clip.frames,fps:source.clip.fps,baseFps:base.clip.fps,duration:source.clip.duration,baseDuration:base.clip.duration,
      sourceMetadata:source.metadataPath,sourceMetadataSha256:source.metadataSha256,baseMetadata:base.metadataPath,baseMetadataSha256:base.metadataSha256,
      originalReferenceRecovered:false,choice:`Explicit Fightnight base selection; same skeleton and retarget. ${pair.baseMode===2?'Duration-scaled base sampling.':'Base frame 0.'} The cooked source does not name RefPoseSeq.`}});
}
const serialized=`{\n  "schemaVersion": 1,\n  "exporter": ${JSON.stringify(exporter)},\n  "bindings": [\n${bindings.map(binding=>'    '+JSON.stringify(binding)).join(',\n')}\n  ]\n}\n`;
for(const file of ['docs/imported/additive-registry.json','dist/assets/imported/additive-registry.json'])fs.writeFileSync(path.join(root,file),serialized);
console.log(`VERIFIED ${bindings.length} explicit additive/base pairs; every source SHA-256 and metadata identity matched.`);
