// Small, read-only inspection of selected recovered clips; never scans the conversion JSONL.
import fs from 'node:fs';
import crypto from 'node:crypto';
import {parseGLB,nodeMatrix} from '../web/imported/asset-loader.js';
import {parseUEAnim,sampleKeys} from '../web/imported/ueanim.js';
import {multiply} from '../web/imported/math.js';

const modelCatalog=JSON.parse(fs.readFileSync('dist/assets/imported/library.json'));
const animationCatalog=JSON.parse(fs.readFileSync('dist/assets/imported/types/animation.json'));
const payload=e=>fs.readFileSync('assets/imported/'+decodeURIComponent(e.url));
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
const modelEntry=modelCatalog.assets.find(e=>e.label==='SK_M_MALE_Base');
const modelBytes=payload(modelEntry),model=parseGLB(modelBytes);
if(hash(modelBytes)!==modelEntry.sha256)throw new Error('Model SHA mismatch');
const joints=[...new Set(model.rig.skins.flatMap(s=>s.joints))];
const names=joints.map(j=>model.rig.nodes[j].name);
const choices={
  idle:'AssaultRifle_Idle_Patrol',idlePose:'AssaultRifle_Idle_Patrol_Pose',unarmedIdle:'Idle_Standing_A_CMM',pickaxeIdle:'PickAxe_idle',
  jogForward:'NoWep_Jog_Loop_N',jogBackward:'NoWep_Jog_Loop_S',jogRight:'NoWep_Jog_Loop_E_LFF',jogLeft:'NoWep_Jog_Loop_W_LFF',
  runForward:'NoWep_Run_Loop_N',sprint:'NoWep_TacSprint_Loop_N',crouchIdle:'NoWep_Crouch_Idle_Loop_Short',crouchForward:'NoWep_Crouch_Loop_N',crouchBackward:'NoWep_Crouch_Loop_S',crouchRight:'NoWep_Crouch_Loop_E_LFF',crouchLeft:'NoWep_Crouch_Loop_W_LFF',
  jump:'NoWep_Jump_From_Idle',jumpAdditive:'JumpUp_NoWep_CMM_Additive',fall:'JumpFall_NoWep_CMM_Additive',land:'JumpLand_NoWep_CMM_Additive',
  freefall:'Dive',glide:'HangGlider_Glide',reload:'AssaultRifle_Core_Reload',swing:'PickAxe_Swing1',death:'Death_Fwd',emote:'Emote_ArmWave_CMM_Loop',
  rifleJog:'Jog_N_AssaultRifle_NonTargeted_CMF',ghostJog:'GhostRock_Jog_N_CMM',ghostJogCore:'GhostRock_Jog_N_CMM_Core',
  rifleBase:'AR_NonTargeted_Pose_CMM',aim:'AR_Targeted_Pose_CMM',aimCrouch:'AR_Crouch_Targeted_Pose_CMM',landCore:'JumpLand_NoWep_CMM_Additive_Core',
  aimOffsetCandidate:'AO_AshtonMilo_Targeted_CD',
  swim:'Swim_Sprint',swimSlow:'Swim_Sprint_Slow',swimIdle:'AR_Swim_Idle_NonTargeted_Pose',vehicle:'ATV_Driver_Idle',vehiclePose:'ATV_Driver_Idle_Pose_CMM',vehicleIdle:'ValetVehicle_Sports_Idle_Driver_CMM',
};
function posedBounds(clip) {
  if(clip.additive)return null;
  const tracks=new Map([...clip.tracks].map(([n,t])=>[n.toLowerCase(),t]));
  const bounds=[];
  for(const fraction of [0,.25,.5,.75]){
    const frame=fraction*(clip.frames-1),local=model.rig.local.slice(),world=[];
    model.rig.nodes.forEach((n,j)=>{
      const track=tracks.get((n.name||'').toLowerCase());if(!track)return;
      const p=sampleKeys(track.position,frame,[0,0,0]),q=sampleKeys(track.rotation,frame,[0,0,0,1],true),s=sampleKeys(track.scale,frame,[1,1,1]);
      local[j]=nodeMatrix({translation:n.name.toLowerCase()==='root'?[0,0,0]:[p[0]*.01,p[2]*.01,p[1]*.01],rotation:[q[0],q[2],q[1],-q[3]],scale:[s[0],s[2],s[1]].map((v,i)=>v*(n.scale?.[i]??1))});
    });
    function at(j){return world[j]??(world[j]=model.rig.parents[j]<0?local[j]:multiply(at(model.rig.parents[j]),local[j]));}
    const skins=model.rig.skins.map(s=>s.joints.map((j,i)=>multiply(at(j),s.inverseBind[i])));
    const min=[Infinity,Infinity,Infinity],max=[-Infinity,-Infinity,-Infinity];
    for(const primitive of model.primitives){const {positions,skinGroups}=primitive.sourceGeometry,skin=skins[primitive.skinIndex];
      for(let v=0;v<positions.length/3;v++){
        const p=positions.subarray(v*3,v*3+3),out=[0,0,0];let sum=0;
        for(const {joints,weights} of skinGroups)for(let k=0;k<4;k++){const w=weights[v*4+k];if(!w)continue;sum+=w;const m=skin[joints[v*4+k]];for(let c=0;c<3;c++)out[c]+=(m[c]*p[0]+m[c+4]*p[1]+m[c+8]*p[2]+m[c+12])*w;}
        for(let c=0;c<3;c++){min[c]=Math.min(min[c],out[c]/sum);max[c]=Math.max(max[c],out[c]/sum);}
      }
    }
    bounds.push({fraction,min,max});
  }
  return bounds;
}
const records=[];
for(const [slot,label] of Object.entries(choices)){
  const entries=animationCatalog.filter(e=>e.label===label);
  for(const entry of entries){
    try {
      const bytes=payload(entry);if(hash(bytes)!==entry.sha256)throw new Error('SHA mismatch');
      const clip=parseUEAnim(bytes),casefoldNames=new Set([...clip.tracks.keys()].map(n=>n.toLowerCase())),matched=names.filter(n=>casefoldNames.has(n.toLowerCase())),missing=names.filter(n=>!casefoldNames.has(n.toLowerCase()));
      const firstValues={},casefoldTracks=new Map([...clip.tracks].map(([n,t])=>[n.toLowerCase(),t]));
      for(const name of ['root','pelvis','spine_01','head','hand_r','hand_l','foot_r','ball_r','ik_hand_gun']){
        const track=casefoldTracks.get(name);if(track)firstValues[name]={position:sampleKeys(track.position,0,null),rotation:sampleKeys(track.rotation,0,null,true),scale:sampleKeys(track.scale,0,null)};
      }
      let maxPosition=0,minScale=Infinity,maxScale=0,quaternionMin=Infinity,animatedBoneCount=0;
      for(const track of clip.tracks.values()){
        if(track.position.length>1||track.rotation.length>1||track.scale.length>1)animatedBoneCount++;
        for(const key of track.position)maxPosition=Math.max(maxPosition,Math.hypot(...key.value));
        for(const key of track.scale)for(const v of key.value){minScale=Math.min(minScale,v);maxScale=Math.max(maxScale,v);}
        for(const key of track.rotation)quaternionMin=Math.min(quaternionMin,Math.hypot(...key.value));
      }
      const root=clip.tracks.get('root')??clip.tracks.get('Root');
      const rootStart=root?sampleKeys(root.position,0,[0,0,0]):null,rootEnd=root?sampleKeys(root.position,clip.frames-1,[0,0,0]):null;
      const result={slot,...entry,verifiedSha256:true,frames:clip.frames,fps:clip.fps,duration:clip.duration,objectPath:clip.objectPath,additive:clip.additive,basePose:clip.basePose,reference:clip.reference,referenceFrame:clip.referenceFrame,tracks:clip.tracks.size,matchedBones:matched.length,totalBones:names.length,missingBones:missing,animatedBoneCount,maxPositionCentimetres:maxPosition,minScale,maxScale,quaternionMin,rootStart,rootEnd,posedBoundsWithoutRootMotion:posedBounds(clip),firstValues};
      records.push(result);
      console.log(`${slot}: ${label}: ${clip.frames}@${clip.fps.toFixed(2)}, ${matched.length}/${names.length}, add=${clip.additive}, base=${clip.basePose}, reference=${clip.reference.split('/').at(-1)}, maxP=${maxPosition.toFixed(2)}, scale=${minScale.toFixed(3)}..${maxScale.toFixed(3)}`);
    } catch(error){records.push({slot,...entry,error:error.message});console.log('FAIL',label,error.message);}
  }
}
const boneFacts=Object.fromEntries(names.filter(n=>['root','pelvis','head','spine_01','spine_02','spine_03','hand_r','hand_l','weapon_r','weapon_l','foot_r','ball_r','foot_l','ball_l','ik_hand_gun'].includes(n)).map(name=>{const n=model.metadata.nodes.find(n=>n.name===name);return [name,{index:n.index,position:n.worldMatrix.slice(12,15),worldMatrix:n.worldMatrix}];}));
const alternatives=[];
for(const label of ['F_MED_Ramirez_Fallback']){
  const entry=modelCatalog.assets.find(e=>e.label===label),bytes=payload(entry),alt=parseGLB(bytes);
  if(hash(bytes)!==entry.sha256)throw new Error('Alternative model SHA mismatch');
  const joints=[...new Set(alt.rig.skins.flatMap(s=>s.joints))].map(j=>alt.rig.nodes[j].name);
  alternatives.push({...entry,verifiedSha256:true,vertices:alt.vertexCount,triangles:alt.triangleCount,joints,bounds:alt.bounds,missingBaseBones:names.filter(n=>!joints.includes(n)),notes:'Complete fallback female body/head according to previous reference report; no verified texture association. Missing weapon/IK sockets require hand_r fallback; proportions have not been visually retarget-validated.'});
}
const attachment={preferredBone:'weapon_r',fallbackBone:'hand_r',parentBone:'hand_r',localTranslationMetres:[0.0019749017,-0.0038106742,0.03411153],localRotation:[6.938893e-18,1.2129358e-8,2.7755572e-17,0.9999999],weightedVertexInfluences:0,notes:'weapon_r and ik_hand_gun have zero body vertex influences. A zero weapon_r scale in Dive hides attached weapons; do not invert its singular normal matrix.'};
const recommendations={
  immediateAbsoluteSlots:['rifleBase','aim','aimCrouch','unarmedIdle','pickaxeIdle','jogForward','jogBackward','jogRight','jogLeft','runForward','sprint','crouchIdle','crouchForward','crouchBackward','crouchRight','crouchLeft','jump','freefall','glide','reload','swing','death','emote','swimSlow','swimIdle','vehiclePose','vehicleIdle'],
  requiresAdditiveBase:['idle','rifleJog','ghostJog','jumpAdditive','fall','land','landCore','aimOffsetCandidate','vehicle'],
  rifleMovement:'Use full-body NoWep locomotion for legs and AR_NonTargeted_Pose_CMM or AR_Targeted_Pose_CMM as an upper-body pose, or explicitly resolve the recovered rifle additive tracks against a suitable reference. Reference pose paths are absent from the cooked additive metadata.',
  rootMotion:'Remove the whole root translation when Fightnight simulation controls displacement and jump height. Preserve pelvis/local motion. Most selected root starts are [0,0,0]; inspect rootStart per record (Death_Fwd begins 0.4828cm along UE Y).',
  floorOffset:'Native standing feet are near y=0; do not recenter each pose AABB. Dive and HangGlider_Glide intentionally have elevated body bounds, and crouch lowers the head.',
  timing:'Use each decoded fps and duration: PickAxe_Swing1 is 97 frames at 121.25fps with 0.8s duration; two-frame pose assets are static poses, not useful time-varying idle loops.',
};
const report={generatedUtc:new Date().toISOString(),model:{...modelEntry,verifiedSha256:true,joints:names,bounds:model.bounds},alternatives,boneFacts,attachment,recommendations,notes:['TRS magnitudes and coverage are structural checks, not final visual validation.','Coverage is case-insensitive (Unreal FName); the original reader previously missed Root and Spine_01/02/03.','Additive flags must be interpreted with converter provenance; do not add reference transforms twice. A Core suffix is not proof of an absolute clip; landCore remains additive.','Forward is +Z in this GLTF, from foot->ball vectors and forward clip root motion; Fightnight -Z requires yaw+PI.','Slot mapping is an integration suggestion based on exact recovered names and source paths.']};
fs.writeFileSync('docs/imported/playable-clip-candidates.json','{\n'+Object.entries(report).map(([k,v])=>'  '+JSON.stringify(k)+': '+JSON.stringify(v)).join(',\n')+',\n  "records": [\n'+records.map(r=>'    '+JSON.stringify(r)).join(',\n')+'\n  ]\n}\n');
for(const name of ['root','pelvis','head','hand_r','hand_l','foot_r','ball_r','foot_l','ball_l','ik_hand_gun'])console.log(name,boneFacts[name]?.position);
