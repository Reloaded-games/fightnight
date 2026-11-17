import {verifiedMedia,IMPORTED_CATALOG} from './native-audio.js';
import {parseUEAnim} from './ueanim.js';
import {createAdditiveResolver} from './additive-resolver.js';
import {packCharacter} from './character-adapter.js';

/** Install the locally prepared rig/clip bank; the authored avatar remains available when absent. */
export async function loadDefaultCharacter(fn) {
  const response=await fetch(new URL('character-runtime.json',IMPORTED_CATALOG));
  if(!response.ok)return false;
  const manifest=await response.json();
  const bytes=await verifiedMedia(manifest,new URL(manifest.url,IMPORTED_CATALOG));
  fn.install_imported_character(bytes);return true;
}

/** Reuse the recovered motion bank on an explicitly selected compatible humanoid. */
export async function installPlayableModel(fn,model,onProgress=()=>{},isCurrent=()=>true) {
  const [presetResponse,registryResponse]=await Promise.all([fetch(new URL('character-presets.json',IMPORTED_CATALOG)),fetch(new URL('additive-registry.json',IMPORTED_CATALOG))]);
  if(!presetResponse.ok||!registryResponse.ok)throw new Error('Prepare o banco do personagem antes de trocar o modelo.');
  const presets=await presetResponse.json(),registry=await registryResponse.json();
  const resolver=createAdditiveResolver({entries:presets.animations,registry,loadClip:async entry=>parseUEAnim(await verifiedMedia(entry,new URL(entry.url,IMPORTED_CATALOG)))});
  const clips=[];
  for(const state of presets.states){const entry=presets.animations.find(e=>e.id===state.id);clips.push({slot:state.slot,clip:await resolver.resolve(entry,{model})});}
  const bytes=await packCharacter(model,clips,{onProgress,isCurrent});if(isCurrent())fn.install_imported_character(bytes);
}
