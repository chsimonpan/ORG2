import {describe,it,expect} from "vitest";
import {toStoryline,COLORS} from "./storylineModel";
import type {NodeTopic} from "./storylineModel";
const anchor=(id:string)=>({ownerSessionId:"o",sourceSessionId:"o",branchId:"b",durableMessageId:id,boundaryVersion:1,boundaryHash:"h"+id});
const snap=(n:number)=>({schemaVersion:1 as const,ownerSessionId:"o",revision:1,edges:[],capabilities:{read:true,fork:false,import:false,mutate:false},nodes:Array.from({length:n},(_,i)=>({id:"n"+i,title:"原文"+i,timestamp:1000+i*60000,laneId:"l",kind:"question" as const,anchor:anchor("m"+i)}))});
describe("topic threads (A scheme)",()=>{
 const tp:Record<string,NodeTopic>={n0:{title:"搭环境",thread:"setup",threadName:"环境"},n1:{title:"画丝带",thread:"ui",parentThread:"setup",threadName:"界面"},n2:{title:"调后端",thread:"api",parentThread:"setup",threadName:"后端"},n3:{title:"再调色",thread:"ui"},n4:{title:"测召回",thread:"recall",parentThread:"api",threadName:"召回"}};
 it("every node is placed on its model thread; threads fork from declared parents; one color per thread",()=>{
  const m=toStoryline(snap(5),null,undefined,tp);
  expect(m.threads.map(t=>t.id)).toEqual(["topic:setup","topic:ui","topic:api","topic:recall"]);
  expect(m.threads.map(t=>t.parent)).toEqual([null,"topic:setup","topic:setup","topic:api"]);
  expect(new Set(m.threads.map(t=>t.color)).size).toBe(4);expect(m.threads.every(t=>COLORS.includes(t.color)&&!t.glow)).toBe(true);
  expect(m.events.find(e=>e.id==="n3")?.thread).toBe("topic:ui");expect(m.events.length).toBe(5);
  expect(m.activeThread).toBe("topic:recall");
 });
 it("nodes without a model topic stay on main untitled; unknown parent becomes a root",()=>{
  const m=toStoryline(snap(3),null,undefined,{n1:{title:"孤儿",thread:"x",parentThread:"nope"}});
  expect(m.threads.map(t=>t.id)).toEqual(["main","topic:x"]);expect(m.threads[1].parent).toBeNull();
  expect(m.events.filter(e=>e.thread==="main").length).toBe(2);
 });
});
