METAL_L='#8a929b'
NO_BLADES=[False]
def draw_hat(px,rect,h):
    BLK='#2a2630'; RED='#c83a3a'; GOLD='#e0b43a'; GOLD_D='#a8801f'; GOLD_L='#ffe38a'; BONE='#e8dfc8'
    if h==1:
        rect(9,0,7,6,BLK); rect(9,4,7,1,RED); rect(7,6,11,2,BLK); px(10,1,'#4a4452'); px(10,2,'#4a4452')
    elif h==2:
        rect(8,4,9,4,'#3a6ae0'); rect(8,4,9,1,'#6a96ff'); rect(8,6,9,1,RED); rect(12,2,1,2,METAL_L)
        if not NO_BLADES[0]:
            rect(8,1,4,1,RED); rect(13,1,4,1,GOLD); px(12,1,BLK)
    elif h==4:
        rect(8,3,9,5,GOLD); rect(8,3,9,1,GOLD_L); rect(8,7,9,1,GOLD_D)
        for x in (8,12,16): rect(x,1,1,2,GOLD); px(x,0,GOLD_L)
        px(10,5,'#e04a4a'); px(12,5,'#4ac0e0'); px(14,5,'#e04a4a')
    elif h==10:
        P='#5a3aa8'; PL='#7a5ad0'
        rect(11,0,3,2,P); rect(10,2,5,2,P); rect(9,4,7,2,P); rect(5,6,15,2,P); rect(5,6,15,1,PL)
        px(12,3,GOLD_L); px(10,5,GOLD_L); px(14,2,GOLD_L); rect(9,5,7,1,GOLD_D)
    elif h==13:
        B='#8a5a2e'; BL='#b07a40'
        rect(9,2,7,5,B); rect(9,2,7,1,BL); rect(9,4,7,1,'#4a2e16'); rect(4,6,17,2,B); rect(4,5,2,1,B); rect(19,5,2,1,B); rect(4,6,17,1,BL)
    elif h==15:
        M='#8a929b'; MD='#5a6068'
        rect(8,3,9,5,M); rect(8,3,9,1,'#b8c0c8'); rect(8,7,9,1,MD); rect(12,6,1,4,MD)
        rect(6,4,2,2,BONE); rect(5,2,2,2,BONE); rect(4,0,2,2,BONE); rect(17,4,2,2,BONE); rect(18,2,2,2,BONE); rect(19,0,2,2,BONE)
    elif h==31:
        R='#8a2a2a'; RL='#b84a4a'
        rect(8,3,9,5,R); rect(8,3,9,1,RL); rect(7,7,11,1,'#3a1010'); rect(6,8,3,4,R); rect(16,8,3,4,R)
        rect(10,1,1,2,GOLD); rect(14,1,1,2,GOLD); px(9,0,GOLD); px(15,0,GOLD); rect(12,3,1,3,GOLD)
    elif h==35:
        C='#cfe4f0'; CD='#8fb0c8'
        for (x,y) in [(9,0),(10,0),(11,0),(12,0),(13,0),(14,0),(15,0),(8,1),(16,1),(7,2),(17,2),(6,3),(18,3),(6,4),(18,4),(6,5),(18,5),(6,6),(18,6),(6,7),(18,7),(6,8),(18,8),(6,9),(18,9),(7,10),(17,10),(7,11),(17,11)]: px(x,y,C)
        rect(7,12,11,2,'#e8eef4'); rect(7,12,11,1,'#ffffff')
        for (x,y) in [(9,2),(10,2),(9,3)]: px(x,y,'#ffffff')
        px(8,11,CD); px(16,11,CD)
def draw_gun(px,rect,g,up,t,td):
    GM='#3a3a40'; GL='#6a707a'; WD='#7a5230'; OL='#5a6a3a'; OLD='#46532c'
    rect(13,15+up,4,3,td)
    if g==1:
        rect(16,15+up,7,2,GL); rect(16,17+up,2,3,GM); rect(22,15+up,1,2,'#c0c6cc'); px(18,17+up,GM)
    elif g==2:
        rect(15,16+up,9,2,GM); rect(15,16+up,9,1,GL); rect(13,17+up,4,3,WD); rect(18,18+up,4,1,WD); rect(23,16+up,1,2,'#c0c6cc')
    elif g==3:
        rect(14,16+up,10,1,GM); rect(13,17+up,4,3,WD); rect(17,14+up,5,2,GL); px(17,16+up,GM); px(23,16+up,'#c0c6cc'); rect(15,17+up,5,1,GM)
    elif g==4:
        rect(15,14+up,9,2,GL); rect(15,17+up,9,2,GL); rect(15,16+up,9,1,GM); rect(14,15+up,4,4,GM); rect(12,19+up,5,3,OLD); rect(21,14+up,1,5,GM)
    elif g==10:
        rect(8,11+up,15,4,OL); rect(8,11+up,15,1,'#7a8a50'); rect(22,10+up,2,6,OLD); rect(13,15+up,2,3,GM); px(9,12+up,OLD); px(9,13+up,OLD)
    elif g==6:
        rect(15,15+up,7,4,'#3a2a6a'); rect(15,15+up,7,1,'#6a4ac0'); rect(22,16+up,2,2,'#7fe8ff'); px(17,17+up,'#7fe8ff'); px(19,17+up,'#d07fff'); rect(15,19+up,2,2,GM)

# ---- remaining catalog ----
_old_hat=draw_hat; _old_gun=draw_gun
BLK='#2a2630'; GOLD='#e0b43a'; GOLD_D='#a8801f'; GOLD_L='#ffe38a'; BONE='#e8dfc8'; BONE_D='#b8ad90'; RED='#c83a3a'; WD='#7a5230'
def _cone(px,rect,c1,c2,pom):
    rows=[(12,0,1),(11,1,3),(11,2,3),(10,3,5),(10,4,5),(9,5,7),(9,6,7)]
    for i,(x,y,w) in enumerate(rows): rect(x,y,w,1,c1 if (i//1)%2==0 else c2)
    rect(8,7,9,1,c2); px(12,0,pom); px(11,0,pom); px(13,0,pom)
def _ring(px,rect,y,c,cd):
    for x in range(10,15): px(x,y,c); px(x,y+2,cd)
    for (x,yy) in ((8,y+1),(9,y+1),(15,y+1),(16,y+1)): px(x,yy,c)
def _horns(px,rect,c,cl,big):
    if big:
        for sx in (1,-1):
            xs=lambda x:(x if sx==1 else 24-x-1)
            rect(xs(8) if sx==1 else xs(9),4,2,3,c)
            for (x,y,w,h) in ((6,3,2,2),(5,1,2,2),(4,0,2,1)):
                rect(x if sx==1 else 24-x-w,y,w,h,c)
        rect(9,5,7,2,'#2a1a1a'); rect(9,5,7,1,'#4a2a2a')
    else:
        rect(9,4,2,3,c); rect(8,2,2,2,c); px(8,1,cl); rect(14,4,2,3,c); rect(15,2,2,2,c); px(16,1,cl)
def _hat2(px,rect,h):
    if h==3:
        rect(14,4,3,3,'#e86aa0'); px(15,5,'#ffe060'); px(15,3,'#e86aa0'); px(15,7,'#e86aa0'); px(13,5,'#e86aa0'); px(17,5,'#e86aa0'); rect(12,6,2,1,'#3a9a4a'); rect(9,6,7,2,'#4a3322')
    elif h==5:
        rect(9,2,7,5,'#b8282c'); rect(9,2,7,1,'#e04a4a'); rect(9,6,7,1,'#7a1a1c'); rect(16,2,1,1,GOLD); rect(17,3,1,4,GOLD); px(17,7,GOLD_D)
    elif h==6:
        rect(8,5,10,3,'#3a3a6a'); rect(9,4,8,1,'#3a3a6a'); rect(10,3,5,1,'#3a3a6a'); px(12,2,'#3a3a6a'); rect(15,6,4,2,'#3a3a6a'); rect(9,4,8,1,'#5a5a9a'); rect(8,7,10,1,'#25254a')
    elif h==7: _cone(px,rect,'#e8508a','#ffd23a','#ffffff')
    elif h==12: _cone(px,rect,'#3a6ae0','#a8c8ff','#ffffff')
    elif h==21: _cone(px,rect,'#8a4ad0','#ffd23a','#ff7ac0')
    elif h==8: _ring(px,rect,0,GOLD_L,GOLD)
    elif h==18: _ring(px,rect,1,'#ffffff','#cfe0ff')
    elif h==9: _horns(px,rect,'#c82020','#ff6a6a',False)
    elif h==19: _horns(px,rect,BONE,BONE_D,False)
    elif h==34: _horns(px,rect,'#a01020','#ff4a4a',True)
    elif h==11:
        rect(9,6,7,2,BLK); rect(11,6,3,2,'#8a929b'); px(12,7,BLK); rect(6,8,3,1,BLK); rect(5,9,3,1,BLK); rect(4,10,3,1,BLK); rect(7,7,2,1,BLK)
    elif h==17:
        rect(9,5,7,3,RED); rect(9,5,7,1,'#e86060')
        for (x,y) in ((10,6),(12,6),(14,6),(11,7),(13,7)): px(x,y,'#ffffff')
        rect(7,7,2,2,RED); rect(6,9,2,2,'#9a2a2a')
    elif h==16:
        rect(9,3,7,4,'#3a8a5a'); rect(9,3,7,1,'#5ab07a'); rect(8,6,9,2,'#2a6a44'); px(12,2,'#ffffff'); rect(11,1,3,2,'#ffffff')
        for x in (10,12,14): px(x,5,'#2a6a44')
    elif h==14:
        rect(5,5,15,3,BLK); rect(4,4,3,2,BLK); rect(18,4,3,2,BLK); rect(8,1,9,5,BLK); rect(8,1,9,1,'#4a4452')
        rect(11,3,3,2,'#ffffff'); px(11,3,BLK); px(13,3,BLK); px(12,5,'#ffffff')
    elif h==22:
        rect(6,5,13,3,BLK); rect(9,2,7,4,BLK); rect(9,2,7,1,'#4a4452'); rect(6,7,13,1,GOLD); px(16,1,RED); rect(15,0,2,2,RED); px(17,0,'#ff8a8a')
    elif h==20:
        G='#4a9a3a'; GL='#7ac060'
        for (x,y) in ((8,6),(7,7),(8,8),(7,9),(8,10),(16,6),(17,7),(16,8),(17,9),(16,10),(9,4),(11,3),(13,3),(15,4)):
            rect(x,y,2,2,G); px(x,y,GL)
        rect(9,5,7,1,'#4a3322')
    elif h==23:
        cols=['#ff4a9a','#ff8a3a','#ffd23a','#4ae08a','#3ab0ff','#8a5aff','#ff4a9a']; hs=[3,5,7,8,7,5,3]
        for i,(c,hh) in enumerate(zip(cols,hs)): rect(9+i,7-hh,1,hh,c)
    elif h==24:
        P='#ff7ab0'; PD='#c8487a'
        rect(14,3,3,4,P); rect(18,2,3,5,P); rect(17,4,2,2,PD); rect(14,3,3,1,'#ffb0d0'); rect(18,2,3,1,'#ffb0d0'); rect(15,6,1,3,PD); rect(19,7,1,2,PD)
    elif h==25:
        B='#6a4a2a'; BL='#8a6a3a'
        rect(8,3,9,5,B); rect(8,3,9,1,BL); rect(8,7,9,1,'#3a2a14')
        for i,y in enumerate(range(4,11)): rect(17+(i%2),y,2,1,BLK if i%2 else '#c0a070')
    elif h==26:
        M='#7a828c'; ML='#b8c0c8'; MD='#4a5058'
        rect(8,1,9,11,M); rect(8,1,9,1,ML); rect(8,11,9,1,MD); rect(9,8,7,1,BLK); rect(12,3,1,9,MD); rect(7,3,1,6,MD); rect(17,3,1,6,MD)
        rect(11,0,3,1,RED); rect(12,-1,1,1,RED)
    elif h==27:
        rect(2,6,21,2,'#e0b43a'); rect(2,6,21,1,'#ffe38a'); rect(9,2,7,4,'#e0b43a'); rect(9,2,7,1,'#ffe38a'); rect(9,5,7,1,'#c83a3a')
        for x in range(3,22,3): px(x,7,'#c83a3a')
    elif h==28:
        M='#2a4ae0'; ML='#6a8aff'
        rect(8,4,9,10,M); rect(8,4,9,1,ML); rect(10,8,2,2,'#ffffff'); rect(13,8,2,2,'#ffffff'); px(11,9,BLK); px(14,9,BLK)
        rect(10,12,5,1,'#e0b43a'); rect(11,6,3,1,'#e0b43a'); px(12,5,'#e0b43a')
    elif h==29:
        rect(5,2,15,2,BLK); rect(7,1,11,1,BLK); rect(9,4,7,4,BLK); rect(9,4,7,1,'#4a4452'); rect(19,3,1,1,GOLD); rect(19,4,1,5,GOLD); rect(18,9,3,1,GOLD_D)
    elif h==30:
        rect(8,3,9,5,'#2a6ae0'); rect(8,3,9,1,'#5a96ff'); rect(14,7,7,1,'#1a3a98'); rect(8,7,9,1,'#1a3a98'); px(12,2,'#1a3a98'); rect(11,5,3,1,'#ffffff')
    elif h==32:
        C='#2a2030'; CL='#4a3a58'; G='#e03a6a'
        rect(8,3,9,5,C); rect(8,3,9,1,CL); rect(8,7,9,1,'#15101a')
        for x in (8,12,16): rect(x,1,1,2,C); px(x,0,G)
        px(10,5,G); px(12,5,G); px(14,5,G)
    elif h==33:
        rect(7,3,11,6,GOLD); rect(7,3,11,1,GOLD_L)
        for x in range(8,17,2): rect(x,4,1,5,'#2a5ac8')
        rect(7,9,3,5,GOLD); rect(15,9,3,5,GOLD)
        for y in (10,12): rect(7,y,3,1,'#2a5ac8'); rect(15,y,3,1,'#2a5ac8')
        rect(12,1,1,3,'#3a9a4a'); px(12,0,'#3a9a4a'); px(13,1,'#3a9a4a')
    elif h==36:
        rect(8,0,9,10,BONE); rect(8,0,9,1,'#ffffff'); rect(8,9,9,1,BONE_D)
        rect(9,4,3,3,BLK); rect(14,4,3,3,BLK); px(10,5,'#ff5a2a'); px(15,5,'#ff5a2a'); rect(12,7,1,2,BONE_D)
        for x in range(9,16,2): px(x,10,BONE); px(x,11,BONE)
        rect(6,1,2,3,BONE); rect(5,0,1,2,BONE); rect(17,1,2,3,BONE); rect(19,0,1,2,BONE); rect(11,0,3,1,BONE_D)
    elif h==37:   # Ben's Birthday Hat: tall purple/gold party cone with a star
        P='#6a3aa8'; PL='#8a5ad0'
        rows=[(12,0,1),(11,1,3),(11,2,3),(10,3,5),(10,4,5),(9,5,7),(9,6,7)]
        for i,(x,y,w) in enumerate(rows):
            for j in range(w): px(x+j,y,GOLD if (i+j)%2==0 else P)
        rect(8,7,9,1,GOLD); px(12,0,GOLD_L); px(11,0,GOLD_L); px(13,0,GOLD_L); px(12,-1,GOLD_L)
        for (x,y) in ((11,4),(13,3),(10,6)): px(x,y,'#ffffff')
        px(9,7,'#ff7ac0'); px(12,7,'#4ac0e0'); px(15,7,'#ff7ac0')
    elif h==38:   # Cosmic Crown
        C='#3a2a6a'; CL='#5a4a9a'
        rect(8,3,9,4,C); rect(8,3,9,1,CL); rect(8,7,9,1,GOLD); rect(8,3,9,1,CL)
        for x in (9,12,15): rect(x,3,1,4,GOLD_D)
        for x in (8,12,16): rect(x,1,1,2,GOLD); px(x,0,'#7fe8ff')
        for (x,y) in ((10,4),(14,5),(11,6),(13,4)): px(x,y,'#cfd8ff')
    elif h==39:   # Phoenix Crest: flames on a gold band
        rect(8,7,9,1,GOLD); rect(8,7,9,1,GOLD)
        hs={8:2,9:4,10:6,11:4,12:8,13:4,14:6,15:4,16:2}
        for x,hh in hs.items():
            for i in range(hh):
                y=6-i; col='#d83a1a' if i<hh*0.4 else ('#ff8a1a' if i<hh*0.75 else '#ffd23a')
                px(x,y,col)
    elif h==40:   # Void Wraith Hood (replaces the head)
        H1='#3a2a5a'; H2='#5a4a8a'
        rect(9,0,7,1,H1); rect(8,1,9,13,H1); rect(9,0,7,1,H2); rect(8,1,9,1,H2)
        rect(9,4,7,9,'#14101e')
        rect(10,7,2,2,'#c04aff'); rect(14,7,2,2,'#c04aff'); px(10,7,'#ffffff'); px(14,7,'#ffffff')
        rect(8,13,9,1,'#2a1f44')
    elif h==41:   # Gilded Jester
        P='#6a3aa8'
        for x in range(8,17):
            for y in (5,6,7): px(x,y,GOLD if (x+y)%2==0 else P)
        rect(12,0,1,5,P); px(12,0,RED); px(11,0,RED); px(11,1,RED)
        rect(7,4,2,1,P); rect(6,3,2,1,GOLD); rect(5,2,2,1,P); px(4,2,'#ffd23a'); px(4,3,'#ffd23a')
        rect(16,4,2,1,P); rect(17,3,2,1,GOLD); rect(18,2,2,1,P); px(20,2,RED); px(20,3,RED)
    elif h==42:   # Crimson War Mask (replaces the face)
        R='#c82a2a'; RL='#e85a5a'; RD='#7a1414'
        rect(8,3,9,11,R); rect(8,3,9,1,RL); rect(8,13,9,1,RD)
        rect(8,0,1,4,GOLD); rect(16,0,1,4,GOLD); px(7,0,GOLD_L); px(17,0,GOLD_L)
        rect(9,6,3,1,BLK); rect(13,6,3,1,BLK); px(10,6,'#ffd23a'); px(14,6,'#ffd23a'); px(12,4,GOLD)
        rect(10,10,5,2,BLK)
        for x in (10,12,14): px(x,10,'#ffffff')
    elif h==43:   # Worm Hat: a worm's face
        W='#e8a8a0'; WD='#c07a78'
        rect(8,2,9,12,W); rect(8,13,9,1,WD); rect(8,2,9,1,'#f2c0b8')
        rect(9,2,7,2,'#6a4a3a')
        rect(9,5,3,4,'#ffffff'); rect(13,5,3,4,'#ffffff'); rect(10,6,2,3,BLK); rect(14,6,2,3,BLK); px(10,6,'#ffffff')
        rect(10,11,5,1,WD)
    else:
        _old_hat(px,rect,h)
def draw_hat(px,rect,h): _hat2(px,rect,h)
def _gun2(px,rect,g,up,t,td):
    GM='#3a3a40'; GL='#6a707a'; OLD='#46532c'
    if g not in (5,7,8,9,11,12,13,14,15,16,17,18,19,20,21,22): return _old_gun(px,rect,g,up,t,td)
    rect(13,15+up,4,3,td)
    if g==5:
        rect(14,14+up,9,5,'#44444c'); rect(14,14+up,9,1,'#7a7a84'); rect(22,13+up,2,7,GM); rect(12,19+up,5,3,WD); px(13,22+up,GM)
    elif g==7:
        rect(16,15+up,7,2,GOLD); rect(16,15+up,7,1,GOLD_L); rect(16,17+up,2,3,GOLD_D); rect(22,15+up,1,2,'#fff2b0')
    elif g==8:
        rect(16,15+up,7,1,GL); rect(16,16+up,4,3,'#8a929b'); rect(20,16+up,3,1,GM); rect(15,18+up,3,3,WD); px(17,17+up,GM); px(18,17+up,GM)
    elif g==9:
        rect(15,16+up,8,2,'#6a6a70'); rect(14,18+up,5,4,'#c86a2a'); rect(14,18+up,5,1,'#e8904a'); rect(23,15+up,1,4,'#ff7a1a'); px(24-1,14+up,'#ffd23a'); px(22,15+up,'#ffb03a')
    elif g==11:
        rect(15,15+up,8,3,GL); rect(15,15+up,8,1,'#9aa0aa'); rect(17,18+up,2,4,GM); rect(12,16+up,3,3,WD); px(22,16+up,'#c0c6cc'); rect(22,14+up,1,1,GM)
    elif g==12:
        rect(15,16+up,9,1,'#7a7a82'); rect(14,17+up,5,3,WD); rect(15,15+up,2,1,GOLD); px(16,14+up,GM); rect(19,17+up,3,1,GOLD_D); px(23,16+up,'#c0c6cc')
    elif g==13:
        rect(14,16+up,9,1,WD); rect(21,12+up,1,9,WD); px(22,12+up,GM); px(22,20+up,GM)
        for i in range(4): px(20-i,13+i+up,'#dddddd'); px(20-i,19-i+up,'#dddddd')
        rect(23,16+up,1,1,'#c0c6cc'); rect(14,17+up,3,2,WD)
    elif g==14:
        rect(16,15+up,8,1,GL); rect(16,16+up,4,2,GL); rect(20,16+up,4,1,GM); rect(15,17+up,3,3,BONE); px(17,18+up,BONE_D)
    elif g==15:
        rect(16,15+up,7,3,'#dfe8f0'); rect(16,15+up,7,1,'#ffffff'); rect(17,16+up,5,1,'#40d8ff'); rect(22,15+up,2,2,'#40d8ff'); rect(16,18+up,2,3,'#9aa8b8')
    elif g==16:
        rect(14,16+up,10,1,GOLD); rect(14,16+up,10,1,GOLD); rect(13,17+up,4,3,WD); rect(16,15+up,1,1,GOLD_D); rect(19,15+up,1,3,GOLD_D); rect(22,15+up,1,3,GOLD_D); px(23,16+up,GOLD_L)
    elif g==17:
        rect(14,15+up,9,3,'#4a3a6a'); rect(14,15+up,9,1,'#6a58a0'); rect(15,16+up,2,1,'#40d8ff'); rect(18,16+up,2,1,'#40d8ff'); rect(22,14+up,2,5,'#7a6ac0'); px(23,16+up,'#d07fff'); rect(14,18+up,3,3,GM)
    elif g==18:
        rect(14,14+up,9,5,'#1a1420'); rect(14,14+up,9,1,'#3a2a48'); rect(22,13+up,2,7,'#3a2a48')
        for (x,y) in ((16,16),(19,15),(18,17),(21,17)): px(x,y+up,'#ff5a1a')
        rect(12,19+up,4,3,'#3a2a48')
    elif g==19:
        rect(14,16+up,10,1,'#9ae8ff'); rect(14,15+up,6,1,'#cff4ff'); rect(17,13+up,5,2,'#6a8aa8'); rect(13,17+up,4,3,'#7a5ac8'); px(23,16+up,'#ffffff'); rect(15,17+up,4,1,'#6ac8e8')
    elif g==20:
        rect(14,15+up,8,3,'#6a1a1a'); rect(14,15+up,8,1,'#a02a2a'); rect(21,14+up,3,5,'#4a1010'); px(23,16+up,'#ffb03a'); px(24-1,15+up,'#ff7a1a'); rect(15,18+up,3,3,WD); rect(18,14+up,1,1,'#ffb03a')
    elif g==21:
        rect(16,15+up,7,1,'#8a1a1a'); rect(16,16+up,4,3,'#c82a2a'); rect(20,16+up,3,1,'#5a1010'); rect(15,18+up,3,3,'#4a1010'); px(17,17+up,'#ff7a7a')
    elif g==22:
        rect(14,16+up,10,1,'#9aa0aa'); rect(14,15+up,10,1,'#3a6aff')
        for x in (16,18,20,22): rect(x,14+up,1,4,'#ffe23a')
        rect(13,17+up,4,3,'#2a2a34'); px(23,16+up,'#ffffff')
def draw_gun(px,rect,g,up,t,td): _gun2(px,rect,g,up,t,td)
