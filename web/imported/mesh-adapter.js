// Convert the recovered GLB scene into Fightnight's packed, vertex-coloured mesh.
// The untouched GLB, skeleton, animations and textures remain in the asset library.
export const TARGET_LENGTH = { pistol: .3, smg: .6, rifle: .95, shotgun: 1, sniper: 1.25, rocket: 1.1, pickaxe: 1.2 };

/** Fit geometry to the existing held-item pivot, or stand a decorative prop on the ground. */
export function bakeMesh(model, target = 'prop') {
  if (!model?.primitives?.length || !model.vertexCount || !model.triangleCount) throw new Error('Modelo sem geometria.');
  if (model.vertexCount > 200000 || model.triangleCount > 300000) throw new Error('Modelo grande demais para a partida; o original continua disponível.');
  const size = model.bounds.size;
  const longest = Math.max(...size);
  if (!Number.isFinite(longest) || longest < .001 || model.triangleCount < 3) throw new Error('Malha auxiliar ou placeholder sem volume utilizável.');
  const isProp = target === 'prop';
  if (!isProp && !TARGET_LENGTH[target]) throw new Error('Destino desconhecido.');
  const axis = size.indexOf(longest);
  const scale = isProp ? Math.min(1, 8 / longest) : TARGET_LENGTH[target] / longest;
  const center = model.bounds.center;
  const transform = (x,y,z,normal = false) => {
    if (!normal) { x -= center[0]; y -= isProp ? model.bounds.min[1] : center[1]; z -= center[2]; }
    if (!isProp && axis === 0) [x,z] = [z,-x];
    else if (!isProp && axis === 1) [y,z] = [z,-y];
    return normal ? [x,y,z] : [x*scale,y*scale + (isProp ? 0 : .04),z*scale - (isProp ? 0 : TARGET_LENGTH[target]*.28)];
  };
  const bytes = new Uint8Array(12 + model.vertexCount*32 + model.triangleCount*12);
  const view = new DataView(bytes.buffer);
  bytes.set([70,78,77,49]); view.setUint32(4,model.vertexCount,true); view.setUint32(8,model.triangleCount*3,true);
  let base = 0, indexOffset = 12 + model.vertexCount*32;
  for (const primitive of model.primitives) {
    const factor = primitive.material.baseColorFactor;
    const missingMaterial = !primitive.material.baseColorTexture && factor.slice(0,3).every(v => v === 1);
    const colorFallback = model.sourceColorStats?.allRGBZero;
    for (let i=0;i<primitive.vertices.length/12;i++) {
      const row=primitive.vertices.subarray(i*12,i*12+12), offset=12+(base+i)*32;
      const geometry=[...transform(...row.slice(0,3)),...transform(...row.slice(3,6),true)];
      geometry.forEach((v,k) => { if (!Number.isFinite(v)) throw new Error('Vértice inválido.'); view.setFloat32(offset+k*4,v,true); });
      for (let c=0;c<3;c++) {
        const color=(colorFallback ? 1 : row[8+c])*factor[c]*(missingMaterial ? .68 : 1);
        bytes[offset+24+c]=Math.round(Math.max(0,Math.min(1,color))*255);
      }
      bytes[offset+27]=0; // preserve source colour instead of outfit/rarity tint
      bytes.set([255,0,0,80],offset+28);
    }
    for (const index of primitive.indices) { view.setUint32(indexOffset,base+index,true); indexOffset+=4; }
    base+=primitive.vertices.length/12;
  }
  return bytes;
}

/** Draw the actual recovered triangles as a neutral, rotatable geometry preview. */
export function previewMesh(canvas, model, angle = .6) {
  const ctx=canvas.getContext('2d'), w=canvas.width, h=canvas.height;
  ctx.fillStyle='#111c32';ctx.fillRect(0,0,w,h);
  const scale=Math.min(w,h)*.7/Math.max(...model.bounds.size), c=Math.cos(angle), s=Math.sin(angle);
  const project=(v) => {
    const x=v[0]-model.bounds.center[0], y=v[1]-model.bounds.center[1], z=v[2]-model.bounds.center[2];
    const depth=x*s+z*c;
    return [w/2+(x*c-z*s)*scale,h/2-(y*.94-depth*.34)*scale,depth*.94+y*.34];
  };
  const triangles=[];
  for (const p of model.primitives) {
    for(let i=0;i<p.indices.length;i+=3) {
      const pts=[0,1,2].map(k=>project(p.vertices.subarray(p.indices[i+k]*12,p.indices[i+k]*12+3)));
      triangles.push({pts,shade:Math.round(120+50*Math.abs(p.vertices[p.indices[i]*12+4]))});
    }
  }
  triangles.sort((a,b)=>b.pts.reduce((n,p)=>n+p[2],0)-a.pts.reduce((n,p)=>n+p[2],0));
  for(const {pts,shade} of triangles) {
    ctx.fillStyle=`rgb(${shade},${shade+8},${shade+15})`;ctx.beginPath();
    ctx.moveTo(...pts[0].slice(0,2));ctx.lineTo(...pts[1].slice(0,2));ctx.lineTo(...pts[2].slice(0,2));ctx.closePath();ctx.fill();
  }
}
