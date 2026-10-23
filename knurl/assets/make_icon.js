// Regenerate with: node --preserve-symlinks-main make_icon.js
// No dependencies. All outputs are written beside this script.
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const OUT = __dirname;
const SIZES = [16, 20, 24, 32, 40, 48, 64, 256];
const PALETTES = {
  on: {edge:'#004B87',rim:'#0063B1',groove:'#004F92',ridge:'#2687CC',face:'#0078D4',light:'#2B88D8',pointer:'#FFFFFF',dot:'#58A4DE'},
  off:{edge:'#484848',rim:'#5F5F5F',groove:'#4C4C4C',ridge:'#818181',face:'#8A8A8A',light:'#A0A0A0',pointer:'#EBEBEB',dot:'#A2A2A2'}
};
const rgb = hex => hex.match(/[\da-f]{2}/gi).map(v=>parseInt(v,16));
const mix = (a,b,t) => a.map((v,i)=>v+(b[i]-v)*Math.max(0,Math.min(1,t)));
const point = (c,r,a) => [c+r*Math.sin(a),c-r*Math.cos(a)];
function spec(s) {
  const radius = ({16:6.5,20:8.5,24:10,32:13,40:16,48:19.5,64:26,256:104})[s];
  const rim = ({16:1.9,20:2.2,24:2.4,32:2.7,40:3,48:3.5,64:4.5,256:16})[s];
  return {s,c:s/2,r:radius,f:radius-rim,count:s<=20?8:s===24?16:32,
    ridgeWidth:s===24?1.3:s<=40?1:s===48?1.2:s===64?1.5:4,
    pw:s<=24?2:s<=40?2.5:s===48?3:s===64?4:15,
    dots:s>=48,shadow:s>=32};
}
function segmentDistance(x,y,a,b) {
  const dx=b[0]-a[0],dy=b[1]-a[1];
  const t=Math.max(0,Math.min(1,((x-a[0])*dx+(y-a[1])*dy)/(dx*dx+dy*dy)));
  return Math.hypot(x-a[0]-t*dx,y-a[1]-t*dy);
}
function render(s,variant) {
  const q=spec(s),p=Object.fromEntries(Object.entries(PALETTES[variant]).map(([k,v])=>[k,rgb(v)]));
  const ss=8, data=Buffer.alloc(s*s*4), theta=Math.PI/4;
  const a=point(q.c,q.f*.13,theta),b=point(q.c,q.f*.78,theta);
  const dots=[18,36,54,72].map(d=>point(q.c,q.r+s*.065,d*Math.PI/180));
  const dotR=Math.max(.6,s*.0075), outline=s<=64?1:2;
  for(let py=0;py<s;py++) for(let px=0;px<s;px++) {
    let rr=0,gg=0,bb=0,aa=0;
    for(let sy=0;sy<ss;sy++) for(let sx=0;sx<ss;sx++) {
      const x=px+(sx+.5)/ss,y=py+(sy+.5)/ss;
      const dx=x-q.c,dy=y-q.c,d=Math.hypot(dx,dy);
      let col=[0,0,0],alpha=0;
      if(q.shadow) {
        const sd=Math.hypot(dx,dy-s*.022);
        alpha=.14*Math.exp(-Math.pow(Math.max(0,sd-q.r+s*.025)/(s*.022),2));
      }
      if(d<=q.r) {
        alpha=1;
        const illumination=Math.max(0,Math.min(1,.5-(dx+dy)/(q.r*2.8)));
        col=d>q.r-outline?p.edge:mix(p.rim,p.ridge,illumination*.22);
        if(d<q.r-outline*.5 && d>q.f+.25) {
          let angle=Math.atan2(dx,-dy); if(angle<0) angle+=Math.PI*2;
          const step=Math.PI*2/q.count, nearest=Math.round(angle/step)*step;
          const ra=point(q.c,q.f+Math.max(.5,(q.r-q.f)*.20),nearest);
          const rb=point(q.c,q.r-outline*.65,nearest);
          if(segmentDistance(x,y,ra,rb)<=q.ridgeWidth/2) col=p.groove;
          if(s>=32) {
            const offset=q.ridgeWidth/q.r*.95;
            const ha=point(q.c,q.f+.7,nearest-offset),hb=point(q.c,q.r-outline,nearest-offset);
            if(segmentDistance(x,y,ha,hb)<=q.ridgeWidth/2) col=mix(p.rim,p.ridge,.45+illumination*.45);
          }
        }
        if(d<=q.f) {
          const hd=Math.hypot(dx+q.f*.40,dy+q.f*.46)/(q.f*1.85);
          col=mix(p.face,p.light,Math.max(0,1-hd)*.85);
          // A full-pixel inset bevel on larger frames; no thin decorative strokes.
          if(s>=32 && d>q.f-Math.max(1,s*.006)) col=mix(col,p.rim,.30);
        }
        if(segmentDistance(x,y,a,b)<=q.pw/2) col=p.pointer;
      }
      if(q.dots && dots.some(dot=>Math.hypot(x-dot[0],y-dot[1])<=dotR)) {col=p.dot;alpha=.65;}
      rr+=col[0]*alpha;gg+=col[1]*alpha;bb+=col[2]*alpha;aa+=alpha;
    }
    const i=(py*s+px)*4;
    if(aa) {data[i]=Math.round(rr/aa);data[i+1]=Math.round(gg/aa);data[i+2]=Math.round(bb/aa);data[i+3]=Math.round(255*aa/(ss*ss));}
  }
  return {w:s,h:s,data};
}
// Minimal standard PNG writer (RGBA, filter 0, lossless DEFLATE).
const crcTable=Array.from({length:256},(_,n)=>{for(let k=0;k<8;k++)n=n&1?0xedb88320^(n>>>1):n>>>1;return n>>>0;});
function crc(buf) {let c=0xffffffff;for(const b of buf)c=crcTable[(c^b)&255]^(c>>>8);return (c^0xffffffff)>>>0;}
function chunk(type,data) {const t=Buffer.from(type),n=Buffer.alloc(4),c=Buffer.alloc(4);n.writeUInt32BE(data.length);c.writeUInt32BE(crc(Buffer.concat([t,data])));return Buffer.concat([n,t,data,c]);}
function png(im) {
  const head=Buffer.alloc(13);head.writeUInt32BE(im.w);head.writeUInt32BE(im.h,4);head[8]=8;head[9]=6;
  const raw=Buffer.alloc((im.w*4+1)*im.h);
  for(let y=0;y<im.h;y++)im.data.copy(raw,y*(im.w*4+1)+1,y*im.w*4,(y+1)*im.w*4);
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',head),chunk('IDAT',zlib.deflateSync(raw,{level:9})),chunk('IEND',Buffer.alloc(0))]);
}
function ico(images) {
  const payloads=images.map(png),header=Buffer.alloc(6+images.length*16);header.writeUInt16LE(1,2);header.writeUInt16LE(images.length,4);
  let offset=header.length;
  images.forEach((im,i)=>{const o=6+i*16;header[o]=im.w===256?0:im.w;header[o+1]=im.h===256?0:im.h;header.writeUInt16LE(1,o+4);header.writeUInt16LE(32,o+6);header.writeUInt32LE(payloads[i].length,o+8);header.writeUInt32LE(offset,o+12);offset+=payloads[i].length;});
  return Buffer.concat([header,...payloads]);
}
function svg(variant) {
  const p=PALETTES[variant],q=spec(256),n=v=>v.toFixed(3);
  let ridges='';
  for(let i=0;i<q.count;i++) {
    const angle=i*Math.PI*2/q.count;
    for(const [offset,color,opacity] of [[0,p.groove,1],[-q.ridgeWidth/q.r*.95,p.ridge,.7]]) {
      const a=point(128,q.f+3.2,angle+offset),b=point(128,q.r-1.3,angle+offset);
      ridges+=`    <path d="M ${n(a[0])} ${n(a[1])} L ${n(b[0])} ${n(b[1])}" stroke="${color}" stroke-width="4" opacity="${opacity}"/>\n`;
    }
  }
  const a=point(128,q.f*.13,Math.PI/4),b=point(128,q.f*.78,Math.PI/4);
  const dots=[18,36,54,72].map(d=>{const pt=point(128,q.r+256*.065,d*Math.PI/180);return `  <circle cx="${n(pt[0])}" cy="${n(pt[1])}" r="1.92" fill="${p.dot}" opacity=".65"/>`;}).join('\n');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256" fill="none">
  <title>Knurl${variant==='off'?' paused':''}</title>
  <desc>Top-down rotary knob with 32 machined rim grooves, a diagonal pointer, and four layer dots.</desc>
  <defs>
    <radialGradient id="face" cx="30%" cy="27%" r="93%"><stop stop-color="${p.light}"/><stop offset="1" stop-color="${p.face}"/></radialGradient>
    <linearGradient id="rim" x1="24" y1="24" x2="232" y2="232" gradientUnits="userSpaceOnUse"><stop stop-color="${p.ridge}"/><stop offset=".45" stop-color="${p.rim}"/><stop offset="1" stop-color="${p.groove}"/></linearGradient>
    <filter id="shadow" x="0" y="0" width="256" height="256" filterUnits="userSpaceOnUse"><feGaussianBlur stdDeviation="4.5"/></filter>
  </defs>
  <circle cx="128" cy="133.6" r="98" fill="#000000" opacity=".14" filter="url(#shadow)"/>
  <circle cx="128" cy="128" r="104" fill="${p.edge}"/>
  <circle cx="128" cy="128" r="102" fill="url(#rim)"/>
  <g stroke-linecap="round">
${ridges}  </g>
  <circle cx="128" cy="128" r="88" fill="${p.rim}"/>
  <circle cx="128" cy="128" r="86.5" fill="url(#face)"/>
  <path d="M ${n(a[0])} ${n(a[1])} L ${n(b[0])} ${n(b[1])}" stroke="${p.pointer}" stroke-width="15" stroke-linecap="round"/>
${dots}
</svg>
`;
}
// Tiny built-in bitmap lettering keeps the preview portable and avoids font dependencies.
const FONT={
 'A':['010','101','111','101','101'],'B':['110','101','110','101','110'],'C':['111','100','100','100','111'],'D':['110','101','101','101','110'],
 'F':['111','100','110','100','100'],'V':['101','101','101','101','010'],'Y':['101','101','010','010','010'],
 'E':['111','100','110','100','111'],'G':['111','100','101','101','111'],'H':['101','101','111','101','101'],
 'I':['111','010','010','010','111'],'K':['101','101','110','101','101'],'L':['100','100','100','100','111'],
 'N':['101','111','111','111','101'],'O':['111','101','101','101','111'],'P':['110','101','110','100','100'],
 'R':['110','101','110','101','101'],'S':['111','100','111','001','111'],'T':['111','010','010','010','010'],
 'U':['101','101','101','101','111'],'W':['101','101','111','111','101'],'X':['101','101','010','101','101'],
 'Z':['111','001','010','100','111'],'M':['101','111','111','101','101'],
 '0':['111','101','101','101','111'],'1':['010','110','010','010','111'],'2':['110','001','010','100','111'],
 '3':['110','001','010','001','110'],'4':['101','101','111','001','001'],'5':['111','100','110','001','110'],
 '6':['011','100','111','101','111'],'7':['111','001','010','010','010'],'8':['111','101','111','101','111'],
 '9':['111','101','111','001','110'],':':['000','010','000','010','000'],'/':['001','001','010','100','100'],
 '.':['000','000','000','000','010'],'-':['000','000','111','000','000'],' ':['000','000','000','000','000']};
function canvas(w,h,color) {const data=Buffer.alloc(w*h*4),c=rgb(color);for(let i=0;i<data.length;i+=4){data[i]=c[0];data[i+1]=c[1];data[i+2]=c[2];data[i+3]=255;}return {w,h,data};}
function rect(im,x,y,w,h,color) {const c=rgb(color);for(let j=y;j<y+h;j++)for(let i=x;i<x+w;i++){const k=(j*im.w+i)*4;im.data[k]=c[0];im.data[k+1]=c[1];im.data[k+2]=c[2];im.data[k+3]=255;}}
function label(im,text,x,y,color,scale=2) {for(const ch of text){const glyph=FONT[ch]||FONT[' '];glyph.forEach((row,j)=>[...row].forEach((bit,i)=>{if(bit==='1')rect(im,x+i*scale,y+j*scale,scale,scale,color);}));x+=4*scale;}}
function blit(dst,src,x,y,scale=1) {for(let j=0;j<src.h*scale;j++)for(let i=0;i<src.w*scale;i++){const a=(Math.floor(j/scale)*src.w+Math.floor(i/scale))*4,b=((y+j)*dst.w+x+i)*4,t=src.data[a+3]/255;for(let c=0;c<3;c++)dst.data[b+c]=Math.round(src.data[a+c]*t+dst.data[b+c]*(1-t));}}
function preview(frames) {
  const w=1440,h=930,im=canvas(w,h,'#F3F3F3');rect(im,720,0,720,h,'#202020');
  for(let panel=0;panel<2;panel++) {
    const x=panel*720,ink=panel?'#DEDEDE':'#343434',muted=panel?'#A8A8A8':'#666666';
    label(im,'KNURL',x+28,24,ink,4);
    label(im,panel?'DARK 202020':'LIGHT F3F3F3',x+28,58,muted,2);
    label(im,'NATIVE SIZE / 1:1 PIXELS',x+28,96,ink,2);
    // The 256 px column gets a dedicated, non-overlapping area at the right.
    const centers=[45,89,139,197,263,337,417,572];
    SIZES.forEach((s,i)=>label(im,String(s),x+centers[i]-String(s).length*4,124,muted,2));
    for(let v=0;v<2;v++) {
      const variant=v?'off':'on',cy=v?490:250;
      SIZES.forEach((s,i)=>blit(im,frames[variant][s],x+centers[i]-s/2,cy-s/2));
      label(im,v?'PAUSED':'ENABLED',x+28,cy+68,ink,2);
    }
    label(im,'4X / NEAREST NEIGHBOUR',x+28,630,ink,2);
    [16,20,24].forEach((s,i)=>{
      const left=x+40+i*220;
      label(im,String(s)+' PX',left,662,muted,2);
      blit(im,frames.on[s],left,688,4);
      blit(im,frames.off[s],left+100,688,4);
      label(im,'ON',left,804,ink,2);label(im,'PAUSED',left+100,804,ink,2);
    });
    label(im,'32 RIDGES AT 32 PX AND UP',x+28,862,muted,2);
    label(im,'LAYER DOTS AT 48 PX AND UP',x+28,886,muted,2);
  }
  return im;
}
function validateIco(buf) {
  if(buf.readUInt16LE(2)!==1||buf.readUInt16LE(4)!==8)throw Error('Invalid ICO header');
  SIZES.forEach((s,i)=>{const o=6+16*i,dim=buf[o]||256,len=buf.readUInt32LE(o+8),start=buf.readUInt32LE(o+12);if(dim!==s||start+len>buf.length||!buf.subarray(start,start+8).equals(Buffer.from([137,80,78,71,13,10,26,10])))throw Error('Invalid ICO frame');});
}
function main() {
  const frames={on:{},off:{}};
  fs.mkdirSync(path.join(OUT,'frames'),{recursive:true});
  for(const variant of ['on','off']) {
    for(const s of SIZES) {
      const im=render(s,variant);frames[variant][s]=im;
      if(im.data[3]!==0||im.data[((s-1)*s+s-1)*4+3]!==0)throw Error('Corner transparency failed');
      const p=rgb(PALETTES[variant].pointer);
      if(!Array.from({length:s*s},(_,i)=>i*4).some(i=>im.data[i]===p[0]&&im.data[i+1]===p[1]&&im.data[i+2]===p[2]&&im.data[i+3]===255))throw Error('No opaque pointer pixel');
      fs.writeFileSync(path.join(OUT,'frames',`knurl_${variant}_${s}.png`),png(im));
    }
    const output=ico(SIZES.map(s=>frames[variant][s]));validateIco(output);
    fs.writeFileSync(path.join(OUT,`knob_${variant}.ico`),output);
    fs.writeFileSync(path.join(OUT,variant==='on'?'knurl.svg':'knurl_paused.svg'),svg(variant));
  }
  fs.writeFileSync(path.join(OUT,'preview.png'),png(preview(frames)));
  console.log('Created both SVG masters, two 8-frame ICOs, 16 PNG frames, and preview.png.');
  console.log('Verified ICO sizes and PNG payloads, transparent corners, and opaque pointer pixels in all frames.');
}
main();
