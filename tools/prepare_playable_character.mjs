// Prepare a reproducible local player bank from verified original payloads and explicit base bindings.
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {parseGLB} from '../web/imported/asset-loader.js';
import {parseUEAnim} from '../web/imported/ueanim.js';
import {createAdditiveResolver} from '../web/imported/additive-resolver.js';
import {packCharacter} from '../web/imported/character-adapter.js';

const readJSON=name=>JSON.parse(fs.readFileSync(name,'utf8'));
const candidates=readJSON('docs/imported/playable-clip-candidates.json');
const registry=readJSON('docs/imported/additive-registry.json');
const catalog=readJSON('dist/assets/imported/types/animation.json');
const hash=b=>createHash('sha256').update(b).digest('hex');
function payload(entry){const file=path.resolve('assets/imported',decodeURIComponent(entry.url));const bytes=fs.readFileSync(file);if(bytes.length!==entry.bytes||hash(bytes)!==entry.sha256)throw new Error(`Source mismatch: ${entry.label}`);return bytes;}
const roles={idle:'pickaxeIdle',forward:'jogForward',backward:'jogBackward',left:'jogLeft',right:'jogRight',sprint:'sprint',crouch:'crouchIdle',crouch_forward:'crouchForward',crouch_backward:'crouchBackward',crouch_left:'crouchLeft',crouch_right:'crouchRight',jump:'jump',fall:'fall',land:'land',dive:'freefall',glide:'glide',reload:'reload',swing:'swing',emote:'emote',death:'death',swim:'swim',armed_idle:'idle',vehicle:'vehicleIdle',aim:'aim',aim_crouch:'aimCrouch'};
const modelEntry=candidates.model,model=parseGLB(payload(modelEntry));
const entries=new Map(),states=[],proof=[];
const compact=e=>Object.fromEntries(['id','label','type','format','url','bytes','sha256','supported'].map(k=>[k,e[k]]));
function include(entry){entries.set(entry.id,compact(entry));const binding=registry.bindings.find(b=>b.sourceSha256===entry.sha256);if(binding&&!entries.has(binding.baseId)){const base=catalog.find(e=>e.id===binding.baseId);if(!base)throw new Error('Base absent from catalog');include(base);}}
for(const [slot,role] of Object.entries(roles)){const entry=candidates.records.find(r=>r.slot===role);if(!entry)throw new Error(`Role missing: ${role}`);include(entry);states.push({slot,id:entry.id});}
const resolver=createAdditiveResolver({entries:catalog,registry,loadClip:async entry=>parseUEAnim(payload(entry))});
const clips=[];
for(const state of states){const entry=entries.get(state.id),clip=await resolver.resolve(entry,{model});clips.push({slot:state.slot,clip});proof.push({slot:state.slot,label:entry.label,sourceSha256:entry.sha256,resolution:clip.resolution??{kind:'absolute-source'}});console.log('VERIFIED',state.slot,entry.label,clip.resolution?.kind??'absolute');}
const bytes=await packCharacter(model,clips);
const destination='dist/assets/imported';fs.mkdirSync(destination,{recursive:true});
fs.writeFileSync(destination+'/character.fnchar',bytes);
fs.writeFileSync(destination+'/character-presets.json',JSON.stringify({model:compact(modelEntry),states,animations:[...entries.values()]}));
const runtime={url:'./character.fnchar',sha256:hash(bytes),bytes:bytes.length,model:compact(modelEntry),bones:model.rig.skins[0].joints.length,states:proof};
fs.writeFileSync(destination+'/character-runtime.json',JSON.stringify(runtime));
fs.writeFileSync('docs/imported/playable-character-bank.json',JSON.stringify(runtime,null,2));
console.log(`PACKED ${bytes.length} bytes, ${states.length} states, ${runtime.bones} joints; ${runtime.sha256}`);
