import {multiply,normalize} from './math.js';

export const GLB_LIMITS=Object.freeze({maxBytes:128*1024*1024,maxDecodedBytes:256*1024*1024,maxVertices:1_000_000,maxIndices:6_000_000,maxPrimitives:4096,maxNodes:16_384,maxImageBytes:64*1024*1024});
const IDENTITY=()=>new Float32Array([1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]);
const TYPES={SCALAR:1,VEC2:2,VEC3:3,VEC4:4,MAT2:4,MAT3:9,MAT4:16};
const COMPONENTS={5120:{bytes:1,read:'getInt8',array:Int8Array},5121:{bytes:1,read:'getUint8',array:Uint8Array},5122:{bytes:2,read:'getInt16',array:Int16Array},5123:{bytes:2,read:'getUint16',array:Uint16Array},5125:{bytes:4,read:'getUint32',array:Uint32Array},5126:{bytes:4,read:'getFloat32',array:Float32Array}};
const integer=(n,label,min=0)=>{if(!Number.isSafeInteger(n)||n<min)throw new Error(`GLB: ${label} inválido.`);return n;};
const finite=(n,label)=>{if(!Number.isFinite(n))throw new Error(`GLB: ${label} não é finito.`);return n;};
function index(array,i,label){integer(i,label);if(!Array.isArray(array)||i>=array.length)throw new Error(`GLB: ${label} fora do intervalo.`);return array[i];}
function vector(value,size,fallback,label){const a=value??fallback;if(!Array.isArray(a)||a.length!==size)throw new Error(`GLB: ${label} inválido.`);return a.map(v=>finite(v,label));}
const unit=(n,type)=>type===5120?Math.max(n/127,-1):type===5121?n/255:type===5122?Math.max(n/32767,-1):type===5123?n/65535:type===5125?n/4294967295:n;

/** glTF column-major matrix or TRS, using metres and Y-up without guessed UE conversions. */
export function nodeMatrix(node={}) {
 if(node.matrix!==undefined){const m=vector(node.matrix,16,[], 'node.matrix');if(m[3]!==0||m[7]!==0||m[11]!==0||m[15]!==1)throw new Error('GLB: matriz de nó deve ser afim.');return new Float32Array(m);}
 const t=vector(node.translation,3,[0,0,0],'translation'),s=vector(node.scale,3,[1,1,1],'scale');
 const q=vector(node.rotation,4,[0,0,0,1],'rotation'),length=Math.hypot(...q);
 if(length<1e-8)throw new Error('GLB: quaternion nulo.');
 const [x,y,z,w]=q.map(n=>n/length),xx=x*x,yy=y*y,zz=z*z,xy=x*y,xz=x*z,yz=y*z,wx=w*x,wy=w*y,wz=w*z;
 return new Float32Array([(1-2*(yy+zz))*s[0],2*(xy+wz)*s[0],2*(xz-wy)*s[0],0, 2*(xy-wz)*s[1],(1-2*(xx+zz))*s[1],2*(yz+wx)*s[1],0, 2*(xz+wy)*s[2],2*(yz-wx)*s[2],(1-2*(xx+yy))*s[2],0, ...t,1]);
}
function point(m,x,y,z){return [m[0]*x+m[4]*y+m[8]*z+m[12],m[1]*x+m[5]*y+m[9]*z+m[13],m[2]*x+m[6]*y+m[10]*z+m[14]];}
export function normalMatrix(m) {
 const a=[m[0],m[1],m[2]],b=[m[4],m[5],m[6]],c=[m[8],m[9],m[10]];
 const cross=(u,v)=>[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]];
 const bc=cross(b,c),ca=cross(c,a),ab=cross(a,b),det=a[0]*bc[0]+a[1]*bc[1]+a[2]*bc[2];
 if(Math.abs(det)<1e-12)throw new Error('GLB: transformação singular não pode transformar normais.');
 return new Float32Array([...bc.map(n=>n/det),...ca.map(n=>n/det),...ab.map(n=>n/det)]);
}
function normal(m,x,y,z){return normalize([m[0]*x+m[3]*y+m[6]*z,m[1]*x+m[4]*y+m[7]*z,m[2]*x+m[5]*y+m[8]*z]);}
function elementLayout(type,bytes) {
 const count=TYPES[type];if(!count)throw new Error(`GLB: tipo de accessor ${type} não suportado.`);
 if(type.startsWith('MAT')){const size=Number(type.slice(3)),columnStride=Math.ceil(size*bytes/4)*4;return {count,bytes:columnStride*size,offsets:Array.from({length:count},(_,i)=>Math.floor(i/size)*columnStride+(i%size)*bytes)};}
 return {count,bytes:count*bytes,offsets:Array.from({length:count},(_,i)=>i*bytes)};
}

function generatedNormals(positions,indices) {
 const result=new Float32Array(positions.length);
 for(let i=0;i<indices.length;i+=3){const a=indices[i]*3,b=indices[i+1]*3,c=indices[i+2]*3;
  const u=[positions[b]-positions[a],positions[b+1]-positions[a+1],positions[b+2]-positions[a+2]],v=[positions[c]-positions[a],positions[c+1]-positions[a+1],positions[c+2]-positions[a+2]];
  const n=[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]];
  for(const base of [a,b,c])for(let k=0;k<3;k++)result[base+k]+=n[k];
 }
 for(let i=0;i<result.length;i+=3){const n=normalize([...result.subarray(i,i+3)]);result.set(Math.hypot(...n)>0?n:[0,1,0],i);}
 return result;
}
function triangleIndices(source,mode,count,maxIndices) {
 const raw=source??Uint32Array.from({length:count},(_,i)=>i);
 for(const n of raw)if(!Number.isInteger(n)||n<0||n>=count)throw new Error('GLB: índice de vértice fora do intervalo.');
 if(mode===4){if(raw.length%3)throw new Error('GLB: lista de triângulos incompleta.');if(raw.length>maxIndices)throw new Error('GLB: limite de índices excedido.');return new Uint32Array(raw);}
 const length=Math.max(0,(raw.length-2)*3);if(length>maxIndices)throw new Error('GLB: limite de índices excedido.');
 const out=new Uint32Array(length);
 for(let i=0;i<raw.length-2;i++){
  if(mode===5)out.set(i%2?[raw[i+1],raw[i],raw[i+2]]:[raw[i],raw[i+1],raw[i+2]],i*3);
  else out.set([raw[0],raw[i+1],raw[i+2]],i*3);
 }
 return out;
}

/** Strict bounded GLB 2 loader. It never fetches URIs or executes anything in an asset. */
export function parseGLB(input,options={}) {
 const limits={...GLB_LIMITS,...options};
 const bytes=input instanceof ArrayBuffer?new Uint8Array(input):ArrayBuffer.isView(input)?new Uint8Array(input.buffer,input.byteOffset,input.byteLength):null;
 if(!bytes||bytes.length<20)throw new Error('GLB: cabeçalho ausente.');
 if(bytes.length>limits.maxBytes)throw new Error('GLB: arquivo excede o limite de memória do visualizador.');
 const header=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
 if(header.getUint32(0,true)!==0x46546c67)throw new Error('GLB: assinatura inválida.');
 if(header.getUint32(4,true)!==2)throw new Error('GLB: somente versão 2 é suportada.');
 if(header.getUint32(8,true)!==bytes.length)throw new Error('GLB: comprimento declarado diferente do arquivo.');
 let json,binary=new Uint8Array(),offset=12,chunkIndex=0,hasBIN=false;
 while(offset<bytes.length){
  if(offset+8>bytes.length)throw new Error('GLB: cabeçalho de chunk truncado.');
  const length=header.getUint32(offset,true),type=header.getUint32(offset+4,true);offset+=8;
  if(length%4||offset+length>bytes.length)throw new Error('GLB: chunk desalinhado ou truncado.');
  const payload=bytes.subarray(offset,offset+length);
  if(chunkIndex===0&&type!==0x4e4f534a)throw new Error('GLB: primeiro chunk deve ser JSON.');
  if(type===0x4e4f534a){if(json!==undefined)throw new Error('GLB: múltiplos chunks JSON.');try{json=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(payload));}catch(error){throw new Error(`GLB: JSON inválido (${error.message}).`);}}
  if(type===0x004e4942){if(hasBIN)throw new Error('GLB: múltiplos chunks BIN.');hasBIN=true;binary=payload;}
  offset+=length;chunkIndex++;
 }
 if(!json||json.asset?.version!=='2.0')throw new Error('GLB: asset glTF 2.0 ausente.');
 const allowedRequired=new Set(['KHR_materials_unlit','KHR_texture_transform','KHR_mesh_quantization']);
 for(const extension of json.extensionsRequired||[])if(!allowedRequired.has(extension))throw new Error(`GLB: extensão obrigatória não suportada: ${extension}.`);
 const warnings=[];
 const buffers=json.buffers||[];
 if(buffers.length>1||buffers[0]?.uri)throw new Error('GLB: buffers externos não suportados neste visualizador.');
 const declared=buffers.length?integer(buffers[0].byteLength,'buffer.byteLength'):0;
 if(declared>binary.length||binary.length-declared>3)throw new Error('GLB: BIN e buffer.byteLength não correspondem.');
 const bin=new DataView(binary.buffer,binary.byteOffset,declared);
 const accessorCache=new Map();let decodedBytes=0;
 function viewAt(i){const view=index(json.bufferViews,i,'bufferView');if(view.buffer!==0)throw new Error('GLB: bufferView não usa o BIN local.');const start=integer(view.byteOffset??0,'bufferView.byteOffset'),length=integer(view.byteLength,'bufferView.byteLength');if(start+length>declared)throw new Error('GLB: bufferView fora do BIN.');return {view,start,length};}
 function readAccessor(i){
  if(accessorCache.has(i))return accessorCache.get(i);
  const accessor=index(json.accessors,i,'accessor'),component=COMPONENTS[accessor.componentType];
  if(!component)throw new Error('GLB: componentType não suportado.');
  const layout=elementLayout(accessor.type,component.bytes),count=integer(accessor.count,'accessor.count');
  if(count>Math.max(limits.maxVertices,limits.maxIndices))throw new Error('GLB: accessor excede o limite de memória.');
  if(accessor.normalized&&accessor.componentType===5126)throw new Error('GLB: accessor float não pode ser normalized.');
  decodedBytes+=count*layout.count*(accessor.normalized?4:component.bytes);if(decodedBytes>limits.maxDecodedBytes)throw new Error('GLB: accessors excedem o limite de memória decodificada.');
  const data=accessor.normalized?new Float32Array(count*layout.count):new component.array(count*layout.count);
  if(accessor.bufferView!==undefined){
   const {view,start,length}=viewAt(accessor.bufferView),local=integer(accessor.byteOffset??0,'accessor.byteOffset'),stride=integer(view.byteStride??layout.bytes,'byteStride',layout.bytes);
   if(stride>252||stride%component.bytes||(start+local)%component.bytes)throw new Error('GLB: accessor desalinhado ou byteStride inválido.');
   const required=count?(count-1)*stride+layout.bytes:0;
   if(local+required>length)throw new Error('GLB: accessor fora do bufferView.');
   for(let row=0;row<count;row++)for(let c=0;c<layout.count;c++){
    const value=bin[component.read](start+local+row*stride+layout.offsets[c],true);
    data[row*layout.count+c]=accessor.normalized?unit(value,accessor.componentType):value;
   }
  }else if(accessor.byteOffset)throw new Error('GLB: accessor sem bufferView possui offset.');
  if(accessor.sparse){
   const sparse=accessor.sparse,n=integer(sparse.count,'sparse.count');if(n>count)throw new Error('GLB: sparse.count excede accessor.');
   const ic=COMPONENTS[sparse.indices?.componentType];if(![5121,5123,5125].includes(sparse.indices?.componentType))throw new Error('GLB: índices sparse inválidos.');
   const iv=viewAt(sparse.indices.bufferView),io=integer(sparse.indices.byteOffset??0,'sparse.indices.byteOffset');
   const vv=viewAt(sparse.values?.bufferView),vo=integer(sparse.values.byteOffset??0,'sparse.values.byteOffset');
   if(io+n*ic.bytes>iv.length||vo+n*layout.bytes>vv.length)throw new Error('GLB: sparse fora do bufferView.');
   let previous=-1;
   for(let row=0;row<n;row++){
    const target=bin[ic.read](iv.start+io+row*ic.bytes,true);
    if(target>=count||target<=previous)throw new Error('GLB: índices sparse fora do intervalo ou sem ordem.');previous=target;
    for(let c=0;c<layout.count;c++){const value=bin[component.read](vv.start+vo+row*layout.bytes+layout.offsets[c],true);data[target*layout.count+c]=accessor.normalized?unit(value,accessor.componentType):value;}
   }
  }
  if(accessor.componentType===5126)for(const value of data)finite(value,'valor do accessor');
  const result={data,count,components:layout.count,type:accessor.type,componentType:accessor.componentType,normalized:Boolean(accessor.normalized)};accessorCache.set(i,result);return result;
 }
 const nodes=json.nodes||[];if(nodes.length>limits.maxNodes)throw new Error('GLB: limite de nós excedido.');
 const parents=new Int32Array(nodes.length).fill(-1),local=nodes.map(nodeMatrix),world=new Array(nodes.length),visiting=new Set();
 for(let i=0;i<nodes.length;i++)for(const child of nodes[i].children||[]){index(nodes,child,'node.children');if(parents[child]!==-1)throw new Error('GLB: nó possui mais de um pai.');parents[child]=i;}
 function worldAt(i,depth=0){index(nodes,i,'node');if(world[i])return world[i];if(visiting.has(i))throw new Error('GLB: ciclo na hierarquia.');if(depth>512)throw new Error('GLB: hierarquia de nós excede limite de profundidade.');visiting.add(i);world[i]=parents[i]===-1?local[i]:multiply(worldAt(parents[i],depth+1),local[i]);visiting.delete(i);return world[i];}
 for(let i=0;i<nodes.length;i++)worldAt(i);
 const roots=json.scenes?.length?index(json.scenes,json.scene??0,'scene').nodes||[]:nodes.map((_,i)=>i).filter(i=>parents[i]===-1);
 const active=[],activeSet=new Set();function visit(i,depth=0){index(nodes,i,'scene.node');if(activeSet.has(i))throw new Error('GLB: nó repetido na cena.');if(depth>512)throw new Error('GLB: hierarquia de nós excede limite de profundidade.');active.push(i);activeSet.add(i);for(const child of nodes[i].children||[])visit(child,depth+1);}
 for(const root of roots)visit(root);
 const images=(json.images||[]).map((image,i)=>{
  if(image.bufferView!==undefined){const v=viewAt(image.bufferView);if(v.length>limits.maxImageBytes)throw new Error('GLB: imagem excede limite.');if(!['image/png','image/jpeg'].includes(image.mimeType))warnings.push(`Imagem ${i}: MIME ${image.mimeType||'ausente'} não suportado.`);return {mimeType:image.mimeType,bytes:binary.slice(v.start,v.start+v.length),name:image.name||`image-${i}`};}
  if(typeof image.uri!=='string')throw new Error('GLB: imagem sem dados.');
  return {mimeType:image.mimeType||null,uri:image.uri,name:image.name||`image-${i}`};
 });
 const materials=(json.materials||[]).map((material,i)=>{
  const pbr=material.pbrMetallicRoughness||{},factor=vector(pbr.baseColorFactor,4,[1,1,1,1],'baseColorFactor');
  if(factor.some(v=>v<0||v>1))throw new Error('GLB: baseColorFactor fora de 0–1.');
  let texture=null;
  if(pbr.baseColorTexture){const info=pbr.baseColorTexture,tex=index(json.textures,info.index,'texture'),image=index(images,tex.source,'texture.source');
   const transform=info.extensions?.KHR_texture_transform||{},texCoord=transform.texCoord??info.texCoord??0;
   integer(texCoord,'texture.texCoord');
   texture={imageIndex:tex.source,image,texCoord,sampler:tex.sampler===undefined?{}:index(json.samplers,tex.sampler,'sampler'),offset:vector(transform.offset,2,[0,0],'texture.offset'),scale:vector(transform.scale,2,[1,1],'texture.scale'),rotation:finite(transform.rotation??0,'texture.rotation')};
  }
  const alphaMode=material.alphaMode||'OPAQUE';if(!['OPAQUE','MASK','BLEND'].includes(alphaMode))throw new Error('GLB: alphaMode não suportado.');
  return {name:material.name||`material-${i}`,baseColorFactor:factor,baseColorTexture:texture,alphaMode,alphaCutoff:finite(material.alphaCutoff??.5,'alphaCutoff'),doubleSided:Boolean(material.doubleSided),unlit:Boolean(material.extensions?.KHR_materials_unlit),metallicFactor:pbr.metallicFactor??1,roughnessFactor:pbr.roughnessFactor??1};
 });
 const defaultMaterial={name:'default',baseColorFactor:[1,1,1,1],baseColorTexture:null,alphaMode:'OPAQUE',alphaCutoff:.5,doubleSided:false,unlit:false,metallicFactor:1,roughnessFactor:1};
 const primitives=[],bounds={min:[Infinity,Infinity,Infinity],max:[-Infinity,-Infinity,-Infinity]};
 let vertexCount=0,triangleCount=0,skinned=false;
 const skinCache=new Map();
 function skinAt(i){
  if(skinCache.has(i))return skinCache.get(i);
  const skin=index(json.skins,i,'skin'),joints=skin.joints||[];if(!joints.length||joints.length>1024)throw new Error('GLB: skin.joints inválido.');
  const ib=skin.inverseBindMatrices===undefined?null:readAccessor(skin.inverseBindMatrices);
  if(ib&&(ib.type!=='MAT4'||ib.count!==joints.length))throw new Error('GLB: inverseBindMatrices inválido.');
  const transforms=joints.map((j,k)=>multiply(worldAt(j),ib?ib.data.subarray(k*16,k*16+16):IDENTITY()));
  const result={transforms,normals:transforms.map(normalMatrix)};skinCache.set(i,result);return result;
 }
 for(const ni of active){const node=nodes[ni];if(node.mesh===undefined)continue;const mesh=index(json.meshes,node.mesh,'mesh');
  for(let pi=0;pi<(mesh.primitives||[]).length;pi++){
   const primitive=mesh.primitives[pi],mode=primitive.mode??4;
   if(![4,5,6].includes(mode)){warnings.push(`Nó ${ni}, primitiva ${pi}: topologia ${mode} não renderizada.`);continue;}
   if(primitive.extensions?.KHR_draco_mesh_compression)throw new Error('GLB: compressão Draco não suportada.');
   const attr=primitive.attributes||{},positions=readAccessor(attr.POSITION);
   if(positions.type!=='VEC3'||!positions.count)throw new Error('GLB: POSITION deve ter vértices VEC3.');
   vertexCount+=positions.count;if(vertexCount>limits.maxVertices)throw new Error('GLB: limite de vértices excedido.');
   const ia=primitive.indices===undefined?null:readAccessor(primitive.indices);
   if(ia&&(ia.type!=='SCALAR'||![5121,5123,5125].includes(ia.componentType)||ia.normalized))throw new Error('GLB: accessor de índices inválido.');
   const indices=triangleIndices(ia?.data,mode,positions.count,limits.maxIndices-triangleCount*3);triangleCount+=indices.length/3;
   function attribute(name,type){if(attr[name]===undefined)return null;const a=readAccessor(attr[name]);if(a.type!==type||a.count!==positions.count)throw new Error(`GLB: ${name} não corresponde a POSITION.`);return a.data;}
   const normals=attribute('NORMAL','VEC3')??generatedNormals(positions.data,indices);
   const material=primitive.material===undefined?defaultMaterial:index(materials,primitive.material,'material');
   const texCoord=material.baseColorTexture?.texCoord??0,uv=attribute(`TEXCOORD_${texCoord}`,'VEC2');
   if(material.baseColorTexture&&!uv)warnings.push(`Nó ${ni}, primitiva ${pi}: coordenadas TEXCOORD_${texCoord} ausentes; textura ignorada.`);
   let colors=null;
   if(attr.COLOR_0!==undefined){colors=readAccessor(attr.COLOR_0);if(!['VEC3','VEC4'].includes(colors.type)||colors.count!==positions.count)throw new Error('GLB: COLOR_0 inválido.');}
   const sourceColorStats={present:Boolean(colors),vertexCount:colors?positions.count:0,zeroRGBA:0,blackRGB:0,min:[Infinity,Infinity,Infinity,Infinity],max:[-Infinity,-Infinity,-Infinity,-Infinity]};
   let skin=null,skinGroups=[];
   if(node.skin!==undefined){
    skin=skinAt(node.skin);
    for(let group=0;group<2;group++){const joints=attribute(`JOINTS_${group}`,'VEC4'),weights=attribute(`WEIGHTS_${group}`,'VEC4');if(group===0&&(!joints||!weights)||Boolean(joints)!==Boolean(weights))throw new Error(`GLB: skin sem JOINTS_${group}/WEIGHTS_${group}.`);if(joints)skinGroups.push({joints,weights});}
    skinned=true;
   }
   if(primitive.targets?.length)warnings.push(`Nó ${ni}: morph targets preservados no GLB, sem reprodução.`);
   const worldNormal=skin?null:normalMatrix(world[ni]),vertices=new Float32Array(positions.count*12);
   for(let v=0;v<positions.count;v++){
    const p=positions.data.subarray(v*3,v*3+3),n=normals.subarray(v*3,v*3+3);let transformed,transformedNormal;
    if(skin){
     transformed=[0,0,0];transformedNormal=[0,0,0];let sum=0;
     for(const {joints,weights} of skinGroups)for(let k=0;k<4;k++){const weight=finite(weights[v*4+k],'peso de skin');if(weight<0)throw new Error('GLB: peso de skin negativo.');if(!weight)continue;const joint=integer(joints[v*4+k],'joint');if(joint>=skin.transforms.length)throw new Error('GLB: joint fora de skin.');const sp=point(skin.transforms[joint],...p),sn=normal(skin.normals[joint],...n);sum+=weight;for(let c=0;c<3;c++){transformed[c]+=sp[c]*weight;transformedNormal[c]+=sn[c]*weight;}}
     if(sum<=0)throw new Error('GLB: vértice de skin sem peso.');transformed=transformed.map(x=>x/sum);transformedNormal=normalize(transformedNormal);
    }else{transformed=point(world[ni],...p);transformedNormal=normal(worldNormal,...n);}
    for(let c=0;c<3;c++){finite(transformed[c],'posição');bounds.min[c]=Math.min(bounds.min[c],transformed[c]);bounds.max[c]=Math.max(bounds.max[c],transformed[c]);}
    let tex=uv?[uv[v*2],uv[v*2+1]]:[0,0];const texture=material.baseColorTexture;
    if(texture&&uv){const u=tex[0]*texture.scale[0],w=tex[1]*texture.scale[1],c=Math.cos(texture.rotation),s=Math.sin(texture.rotation);tex=[u*c-w*s+texture.offset[0],u*s+w*c+texture.offset[1]];}
    const color=colors?Array.from(colors.data.subarray(v*colors.components,v*colors.components+colors.components)):[1,1,1,1];if(color.length===3)color.push(1);
    if(colors){for(let c=0;c<4;c++){sourceColorStats.min[c]=Math.min(sourceColorStats.min[c],color[c]);sourceColorStats.max[c]=Math.max(sourceColorStats.max[c],color[c]);}if(color.every(n=>n===0))sourceColorStats.zeroRGBA++;if(color.slice(0,3).every(n=>n===0))sourceColorStats.blackRGB++;}
    const row=[...transformed,...transformedNormal,...tex,...color];for(const value of row)finite(value,'vértice');vertices.set(row,v*12);
   }
   if(!colors){sourceColorStats.min=null;sourceColorStats.max=null;}
   if(primitives.length>=limits.maxPrimitives)throw new Error('GLB: limite de primitivas excedido.');
   primitives.push({vertices,indices,material:material.baseColorTexture&&!uv?{...material,baseColorTexture:null}:material,sourceColorStats,nodeIndex:ni,nodeName:node.name||`node-${ni}`,meshIndex:node.mesh,meshName:mesh.name||`mesh-${node.mesh}`,primitiveIndex:pi,skinIndex:node.skin??null});
  }
 }
 if(!primitives.length||!triangleCount)throw new Error('GLB: nenhuma geometria triangular suportada na cena.');
 bounds.center=bounds.min.map((v,i)=>(v+bounds.max[i])/2);bounds.size=bounds.min.map((v,i)=>bounds.max[i]-v);bounds.radius=Math.hypot(...bounds.size)/2;
 const metadata={generator:json.asset.generator||null,coordinateSystem:'glTF metres, Y-up',displayMode:skinned?'bind-pose':'static',animationPlayback:false,
  nodes:nodes.map((n,i)=>({index:i,name:n.name||`node-${i}`,mesh:n.mesh??null,skin:n.skin??null,children:[...(n.children||[])],worldMatrix:Array.from(world[i])})),
  skins:(json.skins||[]).map((s,i)=>({index:i,name:s.name||`skin-${i}`,joints:[...(s.joints||[])],skeleton:s.skeleton??null,inverseBindMatrices:s.inverseBindMatrices??null})),
  animations:(json.animations||[]).map((a,i)=>({name:a.name||`animation-${i}`,channels:(a.channels||[]).map(c=>({sampler:c.sampler,node:c.target?.node,path:c.target?.path})),samplers:(a.samplers||[]).map(s=>({input:s.input,output:s.output,interpolation:s.interpolation||'LINEAR'}))}))};
 const sourceColorStats={present:primitives.some(p=>p.sourceColorStats.present),vertexCount:primitives.reduce((n,p)=>n+p.sourceColorStats.vertexCount,0),zeroRGBA:primitives.reduce((n,p)=>n+p.sourceColorStats.zeroRGBA,0),blackRGB:primitives.reduce((n,p)=>n+p.sourceColorStats.blackRGB,0)};
 sourceColorStats.allRGBZero=sourceColorStats.present&&sourceColorStats.blackRGB===sourceColorStats.vertexCount;
 return {primitives,materials,images,bounds,vertexCount,triangleCount,meshCount:new Set(primitives.map(p=>p.meshIndex)).size,skinned,sourceColorStats,metadata,warnings};
}

/** Same-origin assets only; preserves resource guards before allocating the final buffer. */
export async function fetchAssetBytes(url,{maxBytes=GLB_LIMITS.maxBytes,fetchImpl=fetch,baseURL=globalThis.location?.href??'http://127.0.0.1/'}={}) {
 const resolved=new URL(url,baseURL),base=new URL(baseURL);
 if(resolved.origin!==base.origin||!['http:','https:'].includes(resolved.protocol))throw new Error('Asset deve estar na mesma origem local do aplicativo.');
 const response=await fetchImpl(resolved.href,{redirect:'error'});
 if(!response.ok)throw new Error(`Falha ao carregar asset: HTTP ${response.status}.`);
 if(response.url&&new URL(response.url).origin!==base.origin)throw new Error('Asset redirecionado para fora da origem local.');
 const declared=Number(response.headers?.get('content-length')||0);
 if(declared>maxBytes)throw new Error('Asset excede o limite de memória do visualizador.');
 if(!response.body?.getReader){const bytes=new Uint8Array(await response.arrayBuffer());if(bytes.length>maxBytes)throw new Error('Asset excede o limite de memória do visualizador.');return bytes;}
 const reader=response.body.getReader(),chunks=[];let length=0;
 try{while(true){const {done,value}=await reader.read();if(done)break;length+=value.byteLength;if(length>maxBytes){await reader.cancel();throw new Error('Asset excede o limite de memória do visualizador.');}chunks.push(value);}}finally{reader.releaseLock();}
 const result=new Uint8Array(length);let offset=0;for(const chunk of chunks){result.set(chunk,offset);offset+=chunk.length;}return result;
}

export async function loadGLB(entry,options={}) {
 if(!entry?.url)throw new Error('Este asset não tem um arquivo compatível disponível.');
 const bytes=await fetchAssetBytes(entry.url,options);
 if(entry.contentHash&&globalThis.crypto?.subtle){const hash=await crypto.subtle.digest('SHA-256',bytes),hex=Array.from(new Uint8Array(hash),b=>b.toString(16).padStart(2,'0')).join('');if(hex.toLowerCase()!==entry.contentHash.toLowerCase())throw new Error('Asset não corresponde ao SHA-256 do manifesto.');}
 const model=parseGLB(bytes,options);
 model.entry={...entry};model.baseURL=new URL(entry.url,options.baseURL??globalThis.location?.href??'http://127.0.0.1/').href;
 if(/SkeletalMesh/i.test(entry.provenance?.className||''))model.metadata.displayMode='bind-pose';
 return model;
}
