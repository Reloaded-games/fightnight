"""Package native Fightnight geometry as standard, self-contained glTF 2.0.

First run the export_assets Rust example. This tool needs only Python's stdlib.
No API calls, account, paid credits, downloaded meshes or external textures.
"""
import argparse
from array import array
from collections import defaultdict
import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[2]
KINDS = ['flat','foliage','grass','wood','brick','shingle','metal','glass','terrain',
         'emissive','skin','cloth','stone','plaster','asphalt','bark','construction']


def linear(c):
    return c / 12.92 if c <= .04045 else ((c + .055) / 1.055) ** 2.4


def default_tint(name):
    if name.startswith(('Pine','Oak','Palm','Birch','Bush')):
        return (.34,.65,.29)
    if name == 'GrassTuft':
        return (.49,.72,.31)
    if name.startswith('Hair'):
        return (.24,.16,.1)
    if name in ('CharHead','CharHand'):
        return (.79,.56,.4)
    if name.startswith('Char'):
        return (.22,.55,.51) if name not in ('CharPelvis','CharLegUp','CharLegLow') else (.38,.43,.29)
    if name.startswith('Vehicle') and name != 'VehicleWheel':
        return (.95,.69,.28)
    return (1,1,1)


def read_mesh(path):
    raw = path.read_bytes()
    magic,nv,ni = struct.unpack_from('<4sII',raw)
    assert magic == b'FNM1' and nv and ni and ni % 3 == 0
    assert len(raw) == 12+nv*32+ni*4
    vertices = list(struct.iter_unpack('<6f8B',raw[12:12+nv*32]))
    indices = struct.unpack_from('<'+'I'*ni,raw,12+nv*32)
    assert max(indices) < nv
    return vertices,indices


class Glb:
    def __init__(self, source):
        self.source = source
        self.binary = bytearray()
        self.doc = {'asset':{'version':'2.0','generator':'Fightnight free asset generator',
                           'copyright':'Original Fightnight assets. MIT; see LICENSE.'},
                    'scene':0,'scenes':[{'name':'Fightnight','nodes':[]}],
                    'nodes':[],'meshes':[],'materials':[],'accessors':[],'bufferViews':[]}
        self.geometry = {}
        self.variants = {}
        for kind in KINDS:
            mat = {'name':kind,'pbrMetallicRoughness':{
                'baseColorFactor':[1,1,1,1], 'metallicFactor':.28 if kind=='metal' else 0,
                'roughnessFactor':.45 if kind in ('metal','glass') else .8},
                'doubleSided':kind in ('foliage','grass','cloth')}
            self.doc['materials'].append(mat)

    def accessor(self, values, component_type, shape, bounds=False):
        widths={'SCALAR':1,'VEC3':3,'VEC4':4}
        values=list(values)
        code = 'f' if component_type==5126 else 'I'
        packed=array(code,values)
        if sys.byteorder != 'little': packed.byteswap()
        self.binary.extend(b'\0' * (-len(self.binary)%4))
        offset=len(self.binary)
        self.binary.extend(packed.tobytes())
        view=len(self.doc['bufferViews'])
        self.doc['bufferViews'].append({'buffer':0,'byteOffset':offset,'byteLength':len(packed)*4,
                                       'target':34963 if shape=='SCALAR' else 34962})
        entry={'bufferView':view,'componentType':component_type,'count':len(values)//widths[shape],'type':shape}
        if bounds:
            entry['min']=[min(values[k::widths[shape]]) for k in range(widths[shape])]
            entry['max']=[max(values[k::widths[shape]]) for k in range(widths[shape])]
        self.doc['accessors'].append(entry)
        return len(self.doc['accessors'])-1

    def mesh(self, name, tint):
        key=(name,tuple(tint))
        if key in self.variants: return self.variants[key]
        if name not in self.geometry:
            vertices,indices=read_mesh(self.source/(name+'.fnmesh'))
            pos=self.accessor((v[k] for v in vertices for k in range(3)),5126,'VEC3',True)
            nrm=self.accessor((v[k] for v in vertices for k in range(3,6)),5126,'VEC3')
            batches=defaultdict(list)
            for i in range(0,len(indices),3):
                batches[vertices[indices[i]][11]].extend(indices[i:i+3])
            batches={kind:self.accessor(idx,5125,'SCALAR') for kind,idx in sorted(batches.items())}
            self.geometry[name]=(vertices,pos,nrm,batches)
        vertices,pos,nrm,batches=self.geometry[name]
        colors=self.accessor((linear(max(0,min(1,v[6+k]/255*(1+(tint[k]-1)*v[9]/255))))
                              for v in vertices for k in range(3)),5126,'VEC3')
        primitives=[{'attributes':{'POSITION':pos,'NORMAL':nrm,'COLOR_0':colors},
                     'indices':idx,'material':kind} for kind,idx in batches.items()]
        mesh=len(self.doc['meshes'])
        self.doc['meshes'].append({'name':name,'primitives':primitives})
        self.variants[key]=mesh
        return mesh

    def node(self,name,position=(0,0,0),yaw=0,scale=1,tint=None):
        tint=default_tint(name) if tint is None else tint
        node={'name':name,'mesh':self.mesh(name,tint),'translation':list(position),
              'rotation':[0,math.sin(yaw/2),0,math.cos(yaw/2)],'scale':[scale]*3}
        self.doc['scenes'][0]['nodes'].append(len(self.doc['nodes']))
        self.doc['nodes'].append(node)

    def save(self,path):
        self.doc['buffers']=[{'byteLength':len(self.binary)}]
        raw=json.dumps(self.doc,separators=(',',':'),allow_nan=False).encode()
        raw+=b' ' * (-len(raw)%4)
        self.binary+=b'\0' * (-len(self.binary)%4)
        data=struct.pack('<4sII',b'glTF',2,28+len(raw)+len(self.binary))
        data+=struct.pack('<II',len(raw),0x4e4f534a)+raw
        data+=struct.pack('<II',len(self.binary),0x004e4942)+self.binary
        path.parent.mkdir(parents=True,exist_ok=True)
        path.write_bytes(data)
        return {'file':str(path.relative_to(OUT)).replace('\\','/'),
                'bytes':len(data),'nodes':len(self.doc['nodes']),
                'sha256':hashlib.sha256(data).hexdigest()}


def main(source):
    manifest={'generator':'Fightnight / Blender + Rust + Python','cost_usd':0,'license':'MIT',
              'units':'metres; Y up; character forward -Z',
              'method':'Procedural local generation; no cloud AI generation or Fortnite assets.',
              'seed':int((source/'seed.txt').read_text()),'files':[]}
    catalog=[line.split('\t') for line in (source/'catalog.tsv').read_text().splitlines()]
    for category,name in catalog:
        glb=Glb(source)
        glb.node(name)
        manifest['files'].append(glb.save(OUT/category/(name+'.glb')))
    for name in ['scout','ranger','pilot','vanguard']:
        path=OUT/'characters'/(name+'.glb')
        path.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(ROOT/'assets/cartoon'/(name+'.glb'),path)
        manifest['files'].append({'file':'characters/'+name+'.glb','bytes':path.stat().st_size,
                                 'rigged':True,'animation_clips':9,
                                 'sha256':hashlib.sha256(path.read_bytes()).hexdigest()})
    print('Exported',len(manifest['files']),'individual GLBs',flush=True)
    glb=Glb(source)
    for line in (source/'island.tsv').read_text().splitlines():
        name,*values=line.split('\t')
        x,y,z,yaw,scale,r,g,b=map(float,values)
        # A small shared palette keeps the full island compact; untinted vertices
        # retain their authored colors. Stored geometry and placements are exact.
        tint=tuple(round(c*8)/8 for c in (r,g,b))
        glb.node(name,(x,y,z),yaw,scale,tint)
    glb.doc['asset']['extras']={'seed':manifest['seed'],'terrainGridMetres':6,
        'purpose':'Editable static island overview; gameplay uses a 3m grid and procedural shaders.'}
    manifest['files'].append(glb.save(OUT/'map'/'island.glb'))
    manifest['individual_models']=len(catalog)+4
    manifest['island_instances']=len(glb.doc['nodes'])
    manifest['limitations']=['Island is an editable static snapshot; terrain uses a 6m overview grid.',
        'Procedural shader textures, wind, water animation and gameplay collisions are not encoded in GLB.',
        'Four rigged outfits share one body design; standalone character parts use local rig pivots.',
        'Map vegetation tint is quantized to a shared palette to reduce file size.']
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    shutil.copy2(ROOT/'LICENSE',OUT/'LICENSE')
    write_archive()
    print('Packaged',manifest['individual_models'],'models and',manifest['island_instances'],'map instances',flush=True)


def write_archive():
    archive=ROOT/'dist-assets'/'fightnight-free-models.zip'
    archive.parent.mkdir(parents=True,exist_ok=True)
    with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as z:
        for path in sorted(OUT.rglob('*')):
            if path.is_file(): z.write(path,'fightnight/'+str(path.relative_to(OUT)))
        z.write(ROOT/'assets/cartoon/fightnight-cartoon.blend','fightnight/source/fightnight-cartoon.blend')
        for path in sorted((ROOT/'tools/assetgen').glob('*.py')):
            z.write(path,'fightnight/source/generators/'+path.name)
        for name in ['characters','weapons','vehicles','environment','buildings','island']:
            path=ROOT/'docs/screenshots'/('models-'+name+'.png')
            if path.exists(): z.write(path,'fightnight/previews/'+path.name)
    print(archive,'(',archive.stat().st_size,'bytes)',flush=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source',type=Path,default=ROOT/'target/asset-export')
    parser.add_argument('--out',type=Path,default=ROOT/'assets/generated')
    parser.add_argument('--archive-only',action='store_true',help='Refresh the ZIP after validation and preview rendering.')
    args=parser.parse_args()
    OUT=args.out.resolve()
    OUT.mkdir(parents=True,exist_ok=True)
    if args.archive_only:
        write_archive()
    else:
        main(args.source)
