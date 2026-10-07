// Portions adapted from context-visualizer bfd2b50d; MIT Copyright(c)2026 Zaheer.
// Complete license and provenance in src/vendor/context-visualizer.
export interface Point {x:number;y:number;w:number;}
export function sample(pts:Point[], step=18){
  if(pts.length<2) return pts.slice();
  const P=[pts[0],...pts,pts[pts.length-1]];
  const out=[];
  for(let i=1;i<P.length-2;i++){
    const p0=P[i-1],p1=P[i],p2=P[i+1],p3=P[i+2];
    for(let s=0;s<step;s++){
      const t=s/step, t2=t*t, t3=t2*t;
      const cr=(a:number,b:number,c:number,d:number)=>0.5*((2*b)+(-a+c)*t+(2*a-5*b+4*c-d)*t2+(-a+3*b-3*c+d)*t3);
      out.push({ x:cr(p0.x,p1.x,p2.x,p3.x), y:cr(p0.y,p1.y,p2.y,p3.y), w:Math.max(1.5,cr(p0.w,p1.w,p2.w,p3.w)) });
    }
  }
  out.push(pts[pts.length-1]);
  return out;
}

// 由中心线 + 粗细 -> 闭合丝带多边形
export function ribbonPath(dense:Point[]){
  if(dense.length===0)return "";
  const top=[], bot=[];
  for(let i=0;i<dense.length;i++){
    const p=dense[i];
    const a=dense[Math.max(0,i-1)], b=dense[Math.min(dense.length-1,i+1)];
    let dx=b.x-a.x, dy=b.y-a.y; const len=Math.hypot(dx,dy)||1; dx/=len; dy/=len;
    const nx=-dy, ny=dx, h=p.w/2;
    top.push([p.x+nx*h, p.y+ny*h]);
    bot.push([p.x-nx*h, p.y-ny*h]);
  }
  let d="M"+top[0][0].toFixed(1)+","+top[0][1].toFixed(1);
  for(let i=1;i<top.length;i++) d+="L"+top[i][0].toFixed(1)+","+top[i][1].toFixed(1);
  for(let i=bot.length-1;i>=0;i--) d+="L"+bot[i][0].toFixed(1)+","+bot[i][1].toFixed(1);
  return d+"Z";
}
export function centerPath(dense:Point[]){
  if(dense.length===0)return "";
  let d="M"+dense[0].x.toFixed(1)+","+dense[0].y.toFixed(1);
  for(let i=1;i<dense.length;i++) d+="L"+dense[i].x.toFixed(1)+","+dense[i].y.toFixed(1);
  return d;
}


export interface Timeline {points:{raw:number;comp:number}[];idle:{from:number;to:number;ms:number}[];width:number;scale:number;at:(raw:number)=>number;}
export function buildTimeline(timestamps:number[]):Timeline{
 if(timestamps.some(t=>!Number.isFinite(t)))throw Error('Invalid canonical timestamp');
 const all=[...new Set(timestamps)].sort((a,b)=>a-b),points:{raw:number;comp:number}[]=[],idle:Timeline['idle']=[];
 if(all.length)points.push({raw:all[0],comp:0});
 for(let i=1;i<all.length;i++){const raw=all[i]-all[i-1],eff=Math.min(Math.max(raw,75000),1320000),comp=points[i-1].comp+eff;if(raw>3000000)idle.push({from:points[i-1].comp,to:comp,ms:raw});points.push({raw:all[i],comp});}
 const total=points.at(-1)?.comp??0,scale=total>0?1720/total:1;
 return {points,idle,width:total>0?160+total*scale+170:330,scale,at:(raw)=>{
  if(!Number.isFinite(raw))throw Error('Invalid canonical timestamp');
  if(points.length<2)return 160;
  if(raw<=points[0].raw)return 160;if(raw>=points[points.length-1].raw)return 160+total*scale;
  let lo=1,hi=points.length-1;while(lo<hi){const mid=(lo+hi)>>1;if(raw<=points[mid].raw)hi=mid;else lo=mid+1;}
  const a=points[lo-1],b=points[lo];return 160+(a.comp+(raw-a.raw)/(b.raw-a.raw)*(b.comp-a.comp))*scale;
 }};
}
export function dataSignature(s:{revision:number;nodes:{id:string;title:string;timestamp:number}[]}){
 return JSON.stringify([s.revision,s.nodes.map(n=>[n.id,n.title,n.timestamp])]);
}
export function starPath(spikes:number,inner:number,outer:number){let d='';const step=Math.PI/spikes;for(let i=0;i<2*spikes;i++){const r=i%2?inner:outer,a=i*step-Math.PI/2;d+=(i?'L':'M')+(Math.cos(a)*r).toFixed(2)+','+(Math.sin(a)*r).toFixed(2);}return d+'Z';}
