// Reader for CUE4Parse's UEFormat v10 attribute layout. Original payloads are never modified.
class Reader {
  constructor(input) { this.bytes=input instanceof Uint8Array?input:new Uint8Array(input);this.view=new DataView(this.bytes.buffer,this.bytes.byteOffset,this.bytes.length);this.offset=0; }
  take(n) { if(!Number.isSafeInteger(n)||n<0||n>this.bytes.length-this.offset)throw new Error('UEAnim truncado ou tamanho inválido.');const b=this.bytes.subarray(this.offset,this.offset+n);this.offset+=n;return b; }
  int() { const at=this.offset;this.take(4);return this.view.getInt32(at,true); }
  float() { const at=this.offset;this.take(4);const n=this.view.getFloat32(at,true);if(!Number.isFinite(n))throw new Error('UEAnim contém número não finito.');return n; }
  byte() { return this.take(1)[0]; }
  string() { const n=this.int();if(n>16384)throw new Error('Nome UEAnim excessivo.');return new TextDecoder('utf-8',{fatal:true}).decode(this.take(n)); }
  done() { if(this.offset!==this.bytes.length)throw new Error('UEAnim contém dados não interpretados.'); }
}

/** Parse bounded native bone tracks; refuse compression/versions we cannot prove. */
export function parseUEAnim(input) {
  const r=new Reader(input);
  if(r.bytes.length>32*1024*1024)throw new Error('Animação excede 32 MB.');
  if(new TextDecoder().decode(r.take(8))!=='UEFORMAT'||r.string()!=='UEANIM')throw new Error('Assinatura UEAnim inválida.');
  const version=r.byte();if(version!==10)throw new Error(`UEAnim versão ${version} não suportada.`);
  const name=r.string(),objectPath=r.string();
  if(r.byte()!==0)throw new Error('UEAnim comprimido requer conversão local.');
  const attributes=new Map(),count=r.int();
  if(count<1||count>32)throw new Error('Atributos UEAnim inválidos.');
  for(let i=0;i<count;i++){const key=r.string();if(attributes.has(key))throw new Error('Atributo UEAnim repetido.');attributes.set(key,r.take(r.int()));}
  r.done();
  if(!attributes.has('METADATA')||!attributes.has('TRACKS'))throw new Error('UEAnim sem metadados ou trilhas.');
  const m=new Reader(attributes.get('METADATA')),frames=m.int(),fps=m.float(),reference=m.string(),additive=m.byte(),basePose=m.byte(),referenceFrame=m.int();m.done();
  if(frames<1||frames>36000||fps<=0||fps>240)throw new Error('Duração UEAnim inválida.');
  const t=new Reader(attributes.get('TRACKS')),trackCount=t.int(),tracks=new Map();let totalKeys=0;
  if(trackCount<1||trackCount>1024)throw new Error('Quantidade de ossos inválida.');
  function keys(components) {
    const count=t.int();totalKeys+=count;
    if(count<0||count>frames||totalKeys>2000000)throw new Error('Quantidade de chaves inválida.');
    const result=[];let previous=-1;
    for(let i=0;i<count;i++){
      const frame=t.int();if(frame<0||frame>=frames||frame<=previous)throw new Error('Chaves UEAnim fora de ordem.');previous=frame;
      const value=Array.from({length:components},()=>t.float());
      if(components===4&&Math.hypot(...value)<1e-8)throw new Error('Quaternion UEAnim nulo.');
      result.push({frame,value});
    }
    return result;
  }
  for(let i=0;i<trackCount;i++){
    const bone=t.string();if(!bone||tracks.has(bone))throw new Error('Osso UEAnim repetido ou vazio.');
    tracks.set(bone,{position:keys(3),rotation:keys(4),scale:keys(3)});
  }
  t.done();if(!totalKeys)throw new Error('UEAnim sem chaves.');
  return {name,objectPath,version,frames,fps,duration:frames/fps,reference,additive,basePose,referenceFrame,tracks};
}

/** Sample native frame indices with linear interpolation and shortest-path quaternion slerp. */
export function sampleKeys(keys,frame,fallback,quaternion=false) {
  if(!keys.length)return fallback;
  let lo=0,hi=keys.length-1;
  while(lo<hi){const mid=Math.ceil((lo+hi)/2);if(keys[mid].frame<=frame)lo=mid;else hi=mid-1;}
  const a=keys[lo],b=keys[Math.min(lo+1,keys.length-1)];
  const t=a===b?0:Math.max(0,Math.min(1,(frame-a.frame)/(b.frame-a.frame)));
  if(!quaternion)return a.value.map((v,i)=>v+(b.value[i]-v)*t);
  const unit=q=>{const n=Math.hypot(...q);return q.map(v=>v/n);};
  const qa=unit(a.value);let qb=unit(b.value),dot=qa.reduce((n,v,i)=>n+v*qb[i],0);
  if(dot<0){qb=qb.map(v=>-v);dot=-dot;}
  if(dot>.9995)return unit(qa.map((v,i)=>v+(qb[i]-v)*t));
  const angle=Math.acos(Math.min(1,dot)),den=Math.sin(angle);
  return qa.map((v,i)=>(v*Math.sin((1-t)*angle)+qb[i]*Math.sin(t*angle))/den);
}
