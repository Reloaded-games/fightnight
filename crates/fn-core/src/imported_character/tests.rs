use super::*;
use crate::game::actor::{Actor,Outfit,MoveMode};

fn fixture()->Vec<u8>{
    fn word(b:&mut Vec<u8>,n:u32){b.extend(n.to_le_bytes());}
    fn float(b:&mut Vec<u8>,n:f32){b.extend(n.to_le_bytes());}
    fn pose(b:&mut Vec<u8>,y:f32){for n in [0.0,y,0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0]{float(b,n);}}
    let mut mesh=b"FNM1".to_vec();word(&mut mesh,3);word(&mut mesh,3);
    for p in [[0.0,0.0,0.0],[1.0,0.0,0.0],[0.0,1.0,0.0]]{for n in p.into_iter().chain([0.0,0.0,1.0]){float(&mut mesh,n);}mesh.extend([180,180,180,0,255,0,0,80]);}
    for n in [0,1,2]{word(&mut mesh,n);}
    let mut b=b"FNC1".to_vec();for n in [3,1,1,mesh.len() as u32]{word(&mut b,n);}for n in [1.0,0.0,std::f32::consts::PI]{float(&mut b,n);}
    for n in [0,1,1,1,2,1,2,2,1,2,2,2,2]{word(&mut b,n);}b.extend(mesh);
    for (parent,y) in [(u32::MAX,0.0),(0,1.0),(1,0.4)]{word(&mut b,parent);pose(&mut b,y);}
    word(&mut b,1);for n in Mat4::from_translation(-Vec3::Y).to_cols_array(){float(&mut b,n);}
    for _ in 0..3 {for _ in 0..8{word(&mut b,0);}for w in [1.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0]{float(&mut b,w);}}
    word(&mut b,0);word(&mut b,2);float(&mut b,2.0);
    for y in [1.0,1.5]{pose(&mut b,0.0);pose(&mut b,y);pose(&mut b,0.4);}
    b
}
fn actor()->Actor{let mut a=Actor::new(0,"native",true,Outfit::preset(0));a.mode=MoveMode::Ground;a.on_ground=true;a.inv.selected=1;a}

#[test]
fn actual_skin_weights_deform_vertices_and_pause_uses_simulation_time(){
    let mut c=decode(&fixture()).unwrap();let a=actor();
    assert!(c.update(&a,0.0,false));let before=c.mesh.verts[0].pos;
    assert!(c.update(&a,0.25,false));assert!(c.mesh.verts[0].pos[1]>before[1]+0.01);
    let last=c.mesh.verts.clone();assert!(!c.update(&a,0.25,false));assert_eq!(c.mesh.verts,last);
    assert!(c.transform(&a).is_finite());assert_eq!(c.bone_count(),1);
}
#[test]
fn controls_select_directional_crouch_air_and_utility_states(){
    let mut a=actor();assert_eq!(motion::motion_slot(&a),0);
    for (v,expected) in [(Vec3::NEG_Z,1),(Vec3::Z,2),(Vec3::NEG_X,3),(Vec3::X,4)]{a.vel=v*4.0;assert_eq!(motion::motion_slot(&a),expected);a.crouching=true;assert_eq!(motion::motion_slot(&a),expected+6);a.crouching=false;}
    a.sprinting=true;assert_eq!(motion::motion_slot(&a),5);a.on_ground=false;a.vel.y=2.0;assert_eq!(motion::motion_slot(&a),11);a.vel.y=-2.0;assert_eq!(motion::motion_slot(&a),12);
    a.mode=MoveMode::Glide;assert_eq!(motion::motion_slot(&a),15);a.mode=MoveMode::Freefall;assert_eq!(motion::motion_slot(&a),14);a.alive=false;assert_eq!(motion::motion_slot(&a),19);
}
#[test]
fn malformed_rigs_never_reach_skinning(){
    let bytes=fixture();for n in [0,4,83,bytes.len()-1]{assert!(decode(&bytes[..n]).is_err());}
    let mut bad=bytes.clone();bad[4..8].copy_from_slice(&u32::MAX.to_le_bytes());assert!(decode(&bad).is_err());
    let mut bad=bytes.clone();bad[20..24].copy_from_slice(&f32::NAN.to_le_bytes());assert!(decode(&bad).is_err());
    let mut bad=bytes.clone();bad[32..36].copy_from_slice(&100u32.to_le_bytes());assert!(decode(&bad).is_err());
    let mut bad=bytes;bad.push(0);assert!(decode(&bad).is_err());
}

#[test]
#[ignore = "requires the verified local extraction; set FN_CHARACTER_BANK"]
fn recovered_player_bank_deforms_every_clip_and_keeps_weapon_hands_attached(){
    use crate::game::items::{WeaponKind,Rarity};
    let bytes=std::fs::read(std::env::var("FN_CHARACTER_BANK").expect("FN_CHARACTER_BANK")).unwrap();
    let mut c=decode(&bytes).unwrap();assert_eq!(c.clips.len(),25);assert_eq!(c.palette.len(),88);
    for index in 0..c.clips.len(){
        let clip=&c.clips[index];let frames=clip.frames;let slot=clip.slot;
        for frame in [0,frames/2,frames-1]{
            c.pose.clone_from_slice(&c.clips[index].poses[frame*c.rest.len()..(frame+1)*c.rest.len()]);c.rebuild_world();c.skin();
            assert!(c.mesh.verts.iter().all(|v|Vec3::from_array(v.pos).is_finite()&&Vec3::from_array(v.pos).length()<5.0&&Vec3::from_array(v.nrm).is_finite()),"{} frame {}",SLOT_NAMES[slot],frame);
        }
        println!("PASS native clip {}: {} frames, bounds {:?}",SLOT_NAMES[slot],frames,c.mesh.bounds());
    }
    let mut a=actor();a.inv.add_weapon(WeaponKind::AssaultRifle,Rarity::Rare,30);a.inv.selected=1;
    for (index,(crouch,pitch)) in [(false,0.0),(false,0.7),(true,0.0)].into_iter().enumerate(){
        a.crouching=crouch;a.anim.crouch=if crouch{1.0}else{0.0};a.ads=true;a.anim.aim=1.0;a.pitch=pitch;c.reset_time();c.update(&a,index as f32,false);
        println!("native hand error crouch={crouch} pitch={pitch}: {} m",c.hand_error);
        assert!(c.hand_error<0.16,"weapon grip unreachable: {}",c.hand_error);
    }
    a.mode=MoveMode::Freefall;a.anim.aim=0.0;c.reset_time();c.update(&a,4.0,false);
    let bounds=c.mesh.bounds();assert!(bounds.max.y-bounds.min.y<0.8,"Dive must be horizontal: {bounds:?}");
}
