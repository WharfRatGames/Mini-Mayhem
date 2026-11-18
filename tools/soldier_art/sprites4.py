from items4 import draw_hat, draw_gun
import math
W,H=24,32
OUT='#15131a'
SKIN='#e7b98a'; SKIN_D='#b98a62'
HELM='#6b7a4a'; HELM_D='#4b5733'; HELM_L='#8b9a64'
PANTS='#4d5543'; PANTS_D='#3a4132'
_B='#2d2a26'; _BL='#47423b'; BOOT=_B; BOOT_L=_BL
TAN='#a58a56'; TAN_D='#7d673c'; TAN_L='#c3a870'
METAL='#5a6068'; METAL_L='#8a929b'; WOOD='#7a5230'
TEAMS={'blue':('#4a7ae8','#2a4aa8','#8fb0ff'),'red':('#e8504a','#a82a26','#ff9b94'),
       'green':('#46b25a','#27793a','#8fe09c'),'orange':('#eb9a3a','#a8601a','#ffc987')}
POSES=['idle']+['walk%d'%i for i in range(8)]+['jump']
HIP=20; FEET=30   # hip row, bottom boot row

def build(variant, team, pose='idle', gun=True, hat=0, boot=None, gunid=0):
    BOOT,BOOT_L=boot if boot else (_B,_BL)
    t,td,tl=team if isinstance(team,tuple) else (TEAMS[team] if team!='x' else ('#ff00ff','#a000a0','#ff80ff'))
    g=[[None]*W for _ in range(H)]
    oy=[0]; skip=[False]
    def px(x,y,c):
        if skip[0]: return
        y+=oy[0]
        if 0<=x<W and 0<=y<H: g[y][x]=c
    def rect(x,y,w,h,c):
        for j in range(h):
            for i in range(w): px(x+i,y+j,c)
    def leg(hx,fx,fy,c,knee=1,roll='flat'):
        # 4px-wide leg from the hip to the foot (fx,fy = top of boot - 1) with a forward-bending knee; boot 6 wide.
        for r in range(HIP,fy+1):
            tt=(r-HIP)/max(1,fy-HIP)
            x=round(hx+(fx-hx)*tt+knee*math.sin(math.pi*tt))
            rect(x,r,4,1,c if (r<fy or boot is None) else BOOT)
        if roll=='heel':
            rect(fx,fy+1,5,3,BOOT); rect(fx+5,fy+1,1,2,BOOT); rect(fx,fy+1,3,1,BOOT_L)
        elif roll=='toe':
            rect(fx,fy+1,5,2,BOOT); rect(fx+5,fy+2,1,2,BOOT); rect(fx+1,fy+3,5,1,BOOT); rect(fx,fy+1,3,1,BOOT_L)
        else:
            rect(fx,fy+1,6,3,BOOT); rect(fx,fy+1,3,1,BOOT_L); rect(fx+5,fy+3,1,1,BOOT_L)
    def foot(p):
        p%=1.0
        if p<0.5:
            r='heel' if p<0.1 else ('toe' if p>=0.4 else 'flat')
            return round(16-12*(p/0.5)),FEET-3,1,r
        s=(p-0.5)/0.5; lift=round(5*math.sin(math.pi*s))
        r='toe' if s<0.25 else ('heel' if s>0.8 else 'flat')
        return round(4+12*s),FEET-3-lift,1+(3 if lift>=3 else 0),r
    def legs(pl,pr):
        if pose=='idle':
            leg(12,14,FEET-3,pr,0); leg(9,7,FEET-3,pl,0); return 0
        if pose=='jump':
            leg(12,15,FEET-6,pr,3); leg(9,6,FEET-5,pl,3); return -1
        k=int(pose[4:]); p=k/8.0
        nx,ny,nk,nr=foot(p); fx2,fy2,fk,fr=foot(p+0.5)
        leg(12,fx2,fy2,pr,fk,fr); leg(9,nx,ny,pl,nk,nr)
        return [0,0,-1,-1,0,0,-1,-1][k]
    K=int(pose[4:]) if pose.startswith('walk') else -1
    d=1 if K in (1,5) else 0
    fl=1 if K in (2,3,6,7) else 0
    up=-1 if pose=='jump' else 0
    v=variant
    if v=='rifleman':
        bob=legs(PANTS,PANTS_D); oy[0]=bob
        rect(3,13+d,5,10,TAN); rect(3,13+d,5,1,TAN_L); rect(3,22+d,5,1,TAN_D); rect(4,16+d,3,1,TAN_D); px(5,18+d,TAN_D)
        rect(7,13,9,9,td); rect(7,13,9,3,t); rect(7,21,9,1,BOOT); px(11,21,METAL_L); px(12,21,METAL_L)
        rect(8,16,3,5,td); rect(8,16,3,1,tl)
        rect(9,8,7,6,SKIN); rect(9,13,7,1,SKIN_D); px(14,10,OUT); px(14,9,SKIN_D); px(16,11,SKIN_D)
        skip[0]=bool(hat)
        rect(8,2,9,7,HELM); rect(8,2,9,1,HELM_L); rect(7,8,10,1,HELM_D); rect(8,5,9,2,t); rect(9,3,3,1,HELM_L)
        skip[0]=False
        skip[0]=(not gun) or bool(gunid)
        rect(13,15+up,4,3,HELM_D); rect(14,16+up,10,2,METAL); rect(23,16+up,1,2,METAL_L); rect(14,18+up,4,2,WOOD); rect(19,14+up,2,2,METAL); px(15,16+up,METAL_L)
    if v=='commando':
        skip[0]=False
        bob=legs(PANTS_D,PANTS); oy[0]=bob
        rect(7,13,9,9,t); rect(7,13,9,2,tl); rect(7,21,9,1,BOOT)
        rect(7,15,3,6,TAN); rect(13,15,3,6,TAN); px(8,17,TAN_L); px(14,17,TAN_L)
        for i in range(6): px(7+i,15+i,TAN_D)
        rect(9,12,7,2,td); rect(7,13,2,1,td); rect(5,14-fl,2,1,td); rect(3,15-fl,2,1,td); rect(2,16-fl,2,2,td)
        rect(9,7,7,6,SKIN); rect(9,10,7,1,SKIN_D); px(14,9,OUT); px(10,7,SKIN_D); px(16,10,SKIN_D)
        skip[0]=bool(hat)
        rect(8,3,9,5,t); rect(6,5,3,2,t); rect(8,3,8,1,tl); px(13,3,TAN); rect(8,7,9,1,td)
        skip[0]=False
        skip[0]=(not gun) or bool(gunid)
        rect(13,15+up,4,3,t); rect(16,15+up,7,3,METAL); px(22,15+up,METAL_L); rect(17,18+up,2,3,BOOT); px(19,14+up,METAL_L); rect(20,14+up,2,1,METAL)
    if v=='trooper':
        skip[0]=False
        bob=legs(PANTS,PANTS_D); oy[0]=bob
        rect(6,13,12,9,HELM_D); rect(6,13,12,3,HELM); rect(9,16,6,5,PANTS_D); px(11,18,METAL_L); px(12,18,METAL_L); px(13,18,METAL_L)
        rect(6,21,12,1,BOOT)
        rect(4,11,5,5,t); rect(4,11,5,1,tl); rect(4,15,5,1,td)
        if hat: rect(9,6,7,8,SKIN); rect(9,13,7,1,SKIN_D); px(14,9,OUT); px(16,10,SKIN_D)
        else:
            rect(8,4,9,7,HELM); rect(8,4,9,1,HELM_L); rect(6,6,2,4,HELM_D); rect(17,6,1,4,HELM_D)
            rect(9,7,8,3,'#1f2b33'); rect(14,7,3,1,'#7fd6ff'); rect(9,10,8,1,SKIN_D)
            rect(11,2,3,2,t); px(12,1,t)
        skip[0]=(not gun) or bool(gunid)
        rect(13,10+up,10,3,METAL); rect(20,8+up,3,6,METAL_L); px(17,10+up,METAL_L); rect(12,12+up,5,3,WOOD)
        rect(12,15+up,5,3,HELM)
    if v=='ranger':
        skip[0]=False
        CL='#5b6a3a'; CD='#46532c'; CLL='#7a8a50'
        bob=legs(PANTS_D,PANTS); oy[0]=bob
        rect(6,12,9,12,CL); rect(6,12,9,1,CLL)
        for x,y in((6,22),(9,22),(12,22),(7,19),(10,18),(12,21),(6,16),(14,23),(8,23)): px(x,y,CD)
        if fl: px(5,21,CL); px(5,22,CD); px(5,23,CD)
        else: px(15,22,CD); px(15,23,CD)
        for x,y in((8,15),(12,14),(10,17),(13,20),(9,19)): px(x,y,CLL)
        rect(7,3,10,8,CL); rect(9,2,6,1,CLL); rect(6,6,2,5,CD); px(7,4,CD)
        rect(12,7,5,5,SKIN); rect(12,11,5,1,t); px(15,8,OUT); rect(12,6,5,1,CD)
        rect(12,15,3,2,t)
        skip[0]=(not gun) or bool(gunid)
        rect(14,15+up,3,3,CD); rect(14,16+up,10,1,METAL_L); rect(13,17+up,4,2,WOOD); rect(16,13+up,6,2,BOOT); px(18,15+up,METAL); px(23,16+up,METAL)
    if v=='sapper':
        skip[0]=False
        HAT='#e0b43a'; HAT_D='#a8801f'
        bob=legs(t,td); oy[0]=bob
        rect(3,14+d,5,7,TAN_D); rect(3,14+d,5,1,TAN); px(5,17+d,TAN)
        rect(7,13,9,9,t); rect(7,13,9,3,TAN); rect(9,16,1,5,td); rect(13,16,1,5,td)
        rect(7,21,9,1,TAN_D); px(9,21,METAL_L); px(13,21,METAL_L); px(11,21,TAN_L)
        rect(9,8,7,6,SKIN); rect(9,13,7,1,SKIN_D); px(14,10,OUT); px(12,12,SKIN_D)
        skip[0]=bool(hat)
        rect(8,3,9,5,HAT); rect(8,3,9,1,'#f2cf62'); rect(6,7,12,1,HAT_D); px(16,5,'#fff7b0'); rect(9,5,5,1,td)
        skip[0]=False
        skip[0]=bool(hat)
        rect(11,3,2,1,'#2b3a45'); rect(13,3,2,1,'#7fd6ff')
        skip[0]=False
        skip[0]=(not gun) or bool(gunid)
        rect(13,15+up,3,3,TAN)
        for (x,y) in((15,15),(16,14),(17,13),(18,12),(19,11),(20,10),(21,9),(21,8)): rect(x,y+up,2,1,WOOD)
        rect(18,5+up,6,2,METAL_L); rect(19,7+up,2,2,METAL); rect(22,7+up,2,2,METAL_L)
    skip[0]=False
    if hat:
        # bare head: short hair so the hat sits on something
        HAIR='#4a3322'
        rect(9,6,7,2,HAIR); rect(8,7,1,3,HAIR)
        draw_hat(px,rect,hat)
    if gunid:
        draw_gun(px,rect,gunid,up,t,td)
    filled=[[g[y][x] is not None for x in range(W)] for y in range(H)]
    for y in range(H):
        for x in range(W):
            if not filled[y][x]:
                for dx,dy in((1,0),(-1,0),(0,1),(0,-1)):
                    nx,ny=x+dx,y+dy
                    if 0<=nx<W and 0<=ny<H and filled[ny][nx]: g[y][x]=OUT;break
    return g
if __name__=='__main__':
    from PIL import Image
    S=4; vs=['rifleman','commando','trooper','ranger','sapper']
    img=Image.new('RGB',(len(POSES)*(W+1)*S,len(vs)*(H+1)*S),(60,40,70))
    for vi,v in enumerate(vs):
        for pi,p in enumerate(POSES):
            g=build(v,'blue',p)
            for y in range(H):
                for x in range(W):
                    c=g[y][x]
                    if c:
                        rgb=tuple(int(c[i:i+2],16) for i in (1,3,5))
                        for j in range(S):
                            for i in range(S): img.putpixel((pi*(W+1)*S+x*S+i,vi*(H+1)*S+y*S+j),rgb)
    img.save('preview6.png')
