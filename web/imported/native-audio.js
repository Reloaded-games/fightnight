import {fetchAssetBytes} from './asset-loader.js';

export const IMPORTED_CATALOG=new URL('../assets/imported/library.json',import.meta.url);

/** Require a manifest hash for every converted or original media payload. */
export async function verifiedMedia(asset,url) {
  if(!/^[a-f0-9]{64}$/i.test(asset.sha256||''))throw new Error('SHA-256 ausente.');
  const bytes=await fetchAssetBytes(url,{maxBytes:96*1024*1024});
  const hash=await crypto.subtle.digest('SHA-256',bytes);
  const hex=Array.from(new Uint8Array(hash),b=>b.toString(16).padStart(2,'0')).join('');
  if(hex!==asset.sha256.toLowerCase())throw new Error('SHA-256 diferente do manifesto de extração');
  return bytes;
}

/** Ask the loopback converter for this catalog ID; no filesystem paths are supplied by the UI. */
export async function loadNativeAudio(asset) {
  const response=await fetch(`/api/imported/audio/${encodeURIComponent(asset.id)}`);
  let result;
  try {result=await response.json();}catch{throw new Error('Inicie o servidor local com python tools/serve_imported.py para converter RADA.');}
  if(!response.ok)throw new Error(result.error||`HTTP ${response.status}`);
  if(result.sourceSha256!==asset.sha256||result.id!==asset.id)throw new Error('Conversão não corresponde ao original selecionado.');
  const bytes=await verifiedMedia(result,new URL(result.url,IMPORTED_CATALOG));
  return {bytes,metadata:result};
}

/** Load a small prepared bank; failure preserves the existing synthesized effect. */
export async function applyNativeSoundDefaults(audio) {
  const response=await fetch(new URL('audio-presets.json',IMPORTED_CATALOG));
  if(!response.ok)return;
  const presets=await response.json();
  for(const preset of presets){
    try {
      const buffers=[];
      for(const entry of preset.assets){const bytes=await verifiedMedia(entry,new URL(entry.url,IMPORTED_CATALOG));buffers.push(await audio.ctx.decodeAudioData(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength)));}
      audio.installImported(preset.slot,buffers);
    } catch(error){console.warn(`Áudio importado ${preset.slot}: ${error.message}`);}
  }
}
