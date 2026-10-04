"""Readable action poses and a shared periodic six-channel locomotion curve.

CLIPS uses seconds. pose() receives normalized clip time; build_assets owns
keyframe insertion and action/NLA creation, using the existing sixteen bones.
Euler axes follow the existing rig's local bone frames, not engine world axes.
"""
import math

CLIPS = [
    ('Idle', 2.4), ('Walk', 1.0), ('Sprint', .8), ('Jump', 1.4),
    ('Crouch', 1.2), ('Reload', 2.2), ('Victory', 2.4),
    ('Aim', 1.2), ('Fire', .8),
]


def motion(phase):
    """hip dip(m), thigh(rad), knee flex(rad), toe-off(rad), roll(rad), sway(m).

    Smooth periodic foot recovery has a planted half and a bent-knee swing half.
    The gameplay rig already applies speed, sprint and crouch blends to these.
    """
    s, c = math.sin(phase), math.cos(phase)
    return (
        .016*(1-math.cos(2*phase)),
        .59*s + .036*math.sin(2*phase) + .025,
        .09 + 1.02*max(c, 0)**2 + .12*max(-s, 0)**2,
        .22*max(-s, 0)**2,
        -.050*s,
        .012*math.sin(phase),
    )


def _smooth(x):
    x = min(1.0, max(0.0, x))
    return x*x*(3-2*x)


def _window(t, start, end, fade=.10):
    return _smooth((t-start)/fade)*_smooth((end-t)/fade)


def pose(rig, clip, t, api):
    """Set a complete deterministic pose, including both endpoint keyframes."""
    t = min(1.0, max(0.0, t))
    ph = math.tau*t
    bones = rig.pose.bones
    for bone in bones:
        bone.rotation_mode = 'XYZ'
        bone.rotation_euler = (0, 0, 0)
        bone.location = (0, 0, 0)
    pelvis, torso, head = bones['pelvis'], bones['torso'], bones['head']

    def rotation(name, x=0, y=0, z=0):
        bones[name].rotation_euler = (x, y, z)

    def arm(side, shoulder, elbow, spread=0, wrist=0, twist=0):
        rotation('upper_arm.'+side, shoulder, twist, spread)
        rotation('forearm.'+side, elbow, 0, 0)
        rotation('hand.'+side, wrist, 0, 0)

    def leg(side, thigh, knee, foot=0, spread=0):
        rotation('thigh.'+side, thigh, 0, spread)
        rotation('shin.'+side, -knee, 0, 0)
        rotation('boot.'+side, foot, 0, 0)

    if clip == 'Idle':
        breath = math.sin(ph)
        pelvis.location.y = .004*breath
        rotation('torso', -.045+.014*breath, .008*math.sin(ph), .014*math.sin(ph))
        rotation('head', .035-.011*breath, 0, .025*math.sin(ph))
        for side, sign in [('L', -1), ('R', 1)]:
            arm(side, .045+.015*breath, .09, sign*.045)
            leg(side, .015, .04, .025)

    elif clip in ('Walk', 'Sprint'):
        amount = 1.0 if clip == 'Walk' else 1.18
        dip, _, _, _, roll, sway = motion(ph)
        pelvis.location.y = -dip*amount
        pelvis.location.x = sway*amount
        rotation('pelvis', 0, .035*math.sin(ph), 0)
        rotation('torso', -.10 if clip == 'Walk' else -.23,
                 -.05*math.sin(ph), roll*amount)
        rotation('head', .045 if clip == 'Walk' else .13, .025*math.sin(ph), -roll*.6)
        for side, offset, sign in [('L', 0, -1), ('R', math.pi, 1)]:
            _, thigh, knee, toe, _, _ = motion(ph+offset)
            thigh *= amount
            foot = max(-.65, min(.65, knee-thigh)) + toe
            leg(side, thigh, knee, foot)
            elbow = .20 if clip == 'Walk' else .74
            arm(side, -.78*thigh, elbow+.10*max(0, math.sin(ph+offset)), sign*.035,
                -.07*math.sin(ph+offset))
        bones['backpack'].rotation_euler.x = .014*math.sin(ph*2)

    elif clip == 'Jump':
        # Anticipation -> take-off -> asymmetric tuck -> landing/recovery.
        compress = _window(t, 0, .27, .11)
        flight = _window(t, .20, .79, .17)
        land = _window(t, .72, 1.0, .11)
        pelvis.location.y = -.11*compress+.35*flight-.13*land
        rotation('torso', -.13*compress-.14*flight-.22*land)
        rotation('head', .05+.10*flight)
        for side, sign in [('L', -1), ('R', 1)]:
            tuck = .76 if side == 'L' else .55
            leg(side, .37*compress+tuck*flight+.44*land,
                .70*compress+1.23*flight+.85*land, .10*flight)
            arm(side, -.22*compress+.76*flight-.08*land,
                .30+.33*flight, sign*(.10+.13*flight))

    elif clip == 'Crouch':
        # Sustained low stance, with an eased entrance and matching exit.
        k = _window(t, 0, 1, .22)
        pelvis.location.y = -.29*k
        rotation('torso', -.34*k+.008*math.sin(ph))
        rotation('head', .20*k)
        for side, sign in [('L', -1), ('R', 1)]:
            leg(side, .93*k, 1.87*k, .91*k)
            arm(side, .32*k, .56*k, sign*.055)

    elif clip in ('Aim', 'Fire', 'Reload'):
        # Shared two-handed weapon stance, without translating the root.
        rotation('torso', -.085, .035, -.035)
        rotation('head', .025, 0, -.025)
        leg('L', .07, .13, .06)
        leg('R', -.04, .06, .10)
        arm('R', .70, 1.01, .04, -.10, -.12)
        arm('L', .93, .96, -.12, .13, .16)
        if clip == 'Aim':
            torso.rotation_euler.x += .012*math.sin(ph)
            head.rotation_euler.x -= .008*math.sin(ph)
            bones['upper_arm.L'].rotation_euler.x += .012*math.sin(ph)
        elif clip == 'Fire':
            # Fast impulse and damped settling within a full 0.8s clip.
            recoil = _window(t, .06, .46, .065)*math.exp(-5*max(0, t-.12))
            settle = .025*math.sin(math.tau*3*t)*_window(t, .20, .90, .12)
            torso.rotation_euler.x += .095*recoil+settle
            bones['upper_arm.R'].rotation_euler.x += .17*recoil
            bones['upper_arm.L'].rotation_euler.x += .12*recoil
            bones['forearm.R'].rotation_euler.x += .12*recoil
            head.rotation_euler.x -= .025*recoil
            bones['backpack'].rotation_euler.x = -.035*recoil
        else:
            lower = _window(t, .06, .87, .15)
            reach = _window(t, .12, .52, .14)
            seat = _window(t, .47, .78, .12)
            rack = _window(t, .68, .92, .09)
            bones['upper_arm.R'].rotation_euler.x -= .22*lower
            bones['forearm.R'].rotation_euler.x += .10*lower
            bones['upper_arm.L'].rotation_euler.x -= .50*reach-.07*seat-.17*rack
            bones['forearm.L'].rotation_euler.x += .46*reach+.28*seat-.30*rack
            bones['upper_arm.L'].rotation_euler.z -= .14*reach
            bones['hand.L'].rotation_euler.x = -.32*reach+.20*seat-.15*rack
            head.rotation_euler.x = -.10*lower
            torso.rotation_euler.z -= .04*reach

    elif clip == 'Victory':
        # Looping celebratory shoulder pump with alternating heel lifts.
        sway = math.sin(ph*2)
        pelvis.location.y = .018*(1-math.cos(ph*4))
        pelvis.location.x = .035*sway
        rotation('pelvis', 0, .13*sway, 0)
        rotation('torso', -.05, -.16*sway, .13*sway)
        rotation('head', .04+.04*math.sin(ph*4), .10*sway, -.08*sway)
        for side, offset, sign in [('L', 0, -1), ('R', math.pi, 1)]:
            beat = math.sin(ph*2+offset)
            arm(side, 2.15+.33*beat, .62+.18*max(0,beat), sign*.19, .16*beat)
            leg(side, .17*max(0,beat), .31*max(0,beat), .11*max(0,-beat))
    else:
        raise ValueError('Unknown Fightnight animation clip: '+str(clip))
