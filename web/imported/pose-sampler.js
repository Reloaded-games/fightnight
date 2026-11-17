import {sampleKeys} from './ueanim.js';

/** Map Unreal FNames without losing case-insensitive matches or accepting ambiguous names. */
export function boneTracks(clip) {
  const tracks=new Map();
  for(const [name,track] of clip.tracks){const key=name.toLowerCase();if(tracks.has(key))throw new Error('Nomes de ossos ambíguos.');tracks.set(key,track);}
  return tracks;
}

/** Sample absolute UE tracks into glTF local TRS. Additive clips must be resolved first. */
export function localPose(model,clip,seconds,tracks=boneTracks(clip)) {
  if(clip.additive)throw new Error('Resolva a pose-base da animação aditiva primeiro.');
  const frame=Math.max(0,Math.min(clip.frames-1,seconds*clip.fps));
  return model.rig.nodes.map(node=>{
    if(node.matrix)throw new Error('Rig com matriz explícita exige conversão para TRS.');
    const pose={translation:node.translation??[0,0,0],rotation:node.rotation??[0,0,0,1],scale:node.scale??[1,1,1]};
    const track=tracks.get(node.name?.toLowerCase());if(!track)return pose;
    if(track.position.length){const [x,y,z]=sampleKeys(track.position,frame);pose.translation=[x*.01,z*.01,y*.01];}
    if(track.rotation.length){const [x,y,z,w]=sampleKeys(track.rotation,frame,null,true);pose.rotation=[x,z,y,-w];}
    if(track.scale.length){const [x,y,z]=sampleKeys(track.scale,frame);pose.scale=[x,z,y].map((v,i)=>v*(node.scale?.[i]??1));}
    return pose;
  });
}
