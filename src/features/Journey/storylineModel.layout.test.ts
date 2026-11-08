import {describe,it,expect} from "vitest";
import {toStoryline,currentEvent,timeText,layoutLabels,textWidth,fitText,overlaps} from "./storylineModel";
import type {Box,LabelReq} from "./storylineModel";
const anchor=(id:string)=>({ownerSessionId:"o",sourceSessionId:"o",branchId:"b",durableMessageId:id,boundaryVersion:1,boundaryHash:"h"+id});
const snap=(n:number)=>({schemaVersion:1 as const,ownerSessionId:"o",revision:1,edges:[],capabilities:{read:true,fork:false,import:false,mutate:false},nodes:Array.from({length:n},(_,i)=>({id:"n"+i,title:"问题"+i,timestamp:1000+i*60000,laneId:"l",kind:"question" as const,anchor:anchor("m"+i)}))});
const dom=(state:string,active?:string)=>({active_branch_id:active,branches:{main:{id:"main",name:"主干",state:"active"},f:{id:"f",name:"fork",parent_branch_id:"main",parent_anchor_message_id:"m1",state}},reviews:{r:{id:"r",fork_id:"f",state:"confirmed"}}});
const visibleBoxes=(ps:{hidden:boolean;box?:Box}[])=>ps.filter(p=>!p.hidden).map(p=>p.box!);
describe("fork layout carries no fabricated time",()=>{
 it("fork/review events keep the anchor's real timestamp and only an order slot",()=>{
  const s=snap(4),m=toStoryline(s,dom("closed"));const real=new Set(s.nodes.map(n=>n.timestamp));
  expect(m.events.every(e=>real.has(e.ts))).toBe(true);
  const f=m.events.find(e=>e.id==="fork:f")!,r=m.events.find(e=>e.id==="review:r")!;
  expect(f.ts).toBe(s.nodes[1].timestamp);expect(r.ts).toBe(s.nodes[1].timestamp);expect([f.seq,r.seq]).toEqual([0,1]);
  expect(m.events.filter(e=>e.thread==="main").every(e=>e.seq===undefined)).toBe(true);
  expect(m.events.map(e=>e.summary).join()).not.toMatch(/布局偏移/);
 });
 it("closed fork without review gets a close step, ordered after the fork at the same anchor time",()=>{
  const d=dom("closed");d.reviews={} as never;const m=toStoryline(snap(3),d);const i=m.events.findIndex(e=>e.id==="fork:f"),j=m.events.findIndex(e=>e.id==="close:f");
  expect(j).toBeGreaterThan(i);expect(m.events[j].ts).toBe(m.events[i].ts);expect(m.events[j].seq).toBe(1);
 });
 it("time text: real events show their time; derived steps say they have no timestamp of their own",()=>{
  const m=toStoryline(snap(3),dom("closed")),f=(t:number)=>"T"+t;
  expect(timeText(m.events.find(e=>e.id==="n2")!,f)).toBe("T"+(1000+2*60000));
  expect(timeText(m.events.find(e=>e.id==="review:r")!,f)).toBe("锚点 T61000 之后（第 2 步；无独立时间戳）");
 });
 it("current node is the latest timestamped event on the active thread, never a derived step",()=>{
  expect(currentEvent(toStoryline(snap(5),dom("active")))?.id).toBe("n4");
  const m=toStoryline(snap(5),dom("active","f"));expect(m.activeThread).toBe("f");expect(currentEvent(m)?.id).toBe("n4");
  expect(toStoryline(snap(2),dom("active","ghost")).activeThread).toBe("main");
  expect(currentEvent(toStoryline(snap(0),null))).toBeUndefined();
 });
});
describe("label collision layout",()=>{
 const opts={fontSize:10,offsets:[-15,24,-28,37],maxWidth:110};
 it("width estimate: CJK is 1em, ascii .6em; fitText truncates with an ellipsis within the budget",()=>{
  expect(textWidth("账单",10)).toBe(20);expect(textWidth("ab",10)).toBe(12);
  const t=fitText("一二三四五六七八九十一二三四五六",60,10);expect(t.endsWith("…")).toBe(true);expect(textWidth(t,10)).toBeLessThanOrEqual(60);expect(fitText("短",60,10)).toBe("短");
 });
 it("40 labels piled on one spot never overlap each other or obstacles; overflow is truncated or tooltip-only",()=>{
  const obstacles:Box[]=[{x0:0,y0:0,x1:2000,y1:40},{x0:95,y0:91,x1:113,y1:109}];
  const reqs:LabelReq[]=Array.from({length:40},(_,i)=>({id:"e"+i,x:100+i*3,y:100,text:"账单导出系统细节设计第"+i+"步",priority:i===39?100:30}));
  const ps=layoutLabels(reqs,{...opts,obstacles});const bs=visibleBoxes(ps);
  expect(bs.length).toBeGreaterThan(3);expect(ps.some(p=>p.hidden)).toBe(true);
  for(let a=0;a<bs.length;a++){for(let b=a+1;b<bs.length;b++)expect(overlaps(bs[a],bs[b])).toBe(false);for(const o of obstacles)expect(overlaps(bs[a],o)).toBe(false);}
  expect(ps.map(p=>p.id)).toEqual(reqs.map(r=>r.id));
  const top=ps[39];expect(top.hidden).toBe(false);expect(top.dy).toBe(-15);
  for(const p of ps){expect(p.full).toBe(reqs.find(r=>r.id===p.id)!.text);if(!p.hidden&&p.truncated)expect(p.text.endsWith("…")).toBe(true);}
 });
 it("sparse labels keep their full text on the first row; long ones are capped at maxWidth",()=>{
  const ps=layoutLabels([{id:"a",x:100,y:100,text:"导出",priority:1},{id:"b",x:400,y:100,text:"非常非常非常非常非常非常非常长的标题",priority:1}],opts);
  expect(ps[0]).toMatchObject({text:"导出",truncated:false,hidden:false,dy:-15});
  expect(ps[1].truncated).toBe(true);expect(ps[1].box!.x1-ps[1].box!.x0).toBeLessThanOrEqual(110+6);
 });
 it("neighbours too close for one row alternate rows instead of overlapping",()=>{
  const ps=layoutLabels(Array.from({length:4},(_,i)=>({id:"k"+i,x:100+i*20,y:100,text:"四个汉字",priority:1})),opts);
  expect(ps.every(p=>!p.hidden&&!p.truncated)).toBe(true);expect(new Set(ps.map(p=>p.dy)).size).toBeGreaterThan(1);
 });
 it("labels are clamped inside horizontal bounds",()=>{
  const [p]=layoutLabels([{id:"z",x:2,y:100,text:"左边缘标题",priority:1}],{...opts,bounds:{x0:0,x1:500}});expect(p.box!.x0).toBeGreaterThanOrEqual(0);
 });
});
