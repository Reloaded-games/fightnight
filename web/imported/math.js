export const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
export const terrainHeight = (x, z) => 0.55 * Math.sin(x * .055) * Math.cos(z * .06) + .18 * Math.sin((x + z) * .15);
export const normalize = v => { const l = Math.hypot(...v) || 1; return v.map(n => n / l); };
export const cross = (a,b) => [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];
export const dot = (a,b) => a[0]*b[0]+a[1]*b[1]+a[2]*b[2];
export function multiply(a,b) {
  const out = new Float32Array(16);
  for (let c=0;c<4;c++) for (let r=0;r<4;r++) {
    out[c*4+r] = a[r]*b[c*4]+a[4+r]*b[c*4+1]+a[8+r]*b[c*4+2]+a[12+r]*b[c*4+3];
  }
  return out;
}
export function perspective(fovDegrees,aspect,near=.1,far=350) {
  const f=1/Math.tan(fovDegrees*Math.PI/360), range=1/(near-far);
  return new Float32Array([f/aspect,0,0,0, 0,f,0,0, 0,0,far*range,-1, 0,0,near*far*range,0]);
}
export function lookAt(eye,target,up=[0,1,0]) {
  const z=normalize(eye.map((v,i)=>v-target[i]));
  let x=normalize(cross(up,z));
  if (Math.hypot(...cross(up,z))<1e-6) x=[1,0,0];
  const y=cross(z,x);
  return new Float32Array([x[0],y[0],z[0],0, x[1],y[1],z[1],0, x[2],y[2],z[2],0, -dot(x,eye),-dot(y,eye),-dot(z,eye),1]);
}
export function compose(position=[0,0,0],scale=[1,1,1],yaw=0) {
  const c=Math.cos(yaw),s=Math.sin(yaw);
  return new Float32Array([c*scale[0],0,-s*scale[0],0, 0,scale[1],0,0, s*scale[2],0,c*scale[2],0, ...position,1]);
}
