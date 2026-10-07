// @vitest-environment jsdom
import {it,expect,vi} from "vitest";import {createElement} from "react";
import {createSmokeRoot,dispatch} from "@src/test/reactSmokeHarness";
import {JourneyOwnerPanel} from "./JourneyPopover";import {loadJourneySnapshot} from "./nativeAdapter";import type {JourneySnapshot} from "./types";
vi.mock("./nativeAdapter",()=>({loadJourneySnapshot:vi.fn()}));
const snap=(owner:string,id:string):JourneySnapshot=>({schemaVersion:1,ownerSessionId:owner,revision:1,nodes:[{id,title:id,timestamp:1000,laneId:`session:${owner}`,kind:"question",anchor:{ownerSessionId:owner,sourceSessionId:owner,branchId:`session:${owner}`,durableMessageId:id,boundaryVersion:1,boundaryHash:`hash-${id}`}}],edges:[],capabilities:{read:true,fork:false,import:false,mutate:false}});
it("mounted canonical entry rejects deferred A1 success/error after A-B-A and selects an explicit historical endpoint",async()=>{
 let a1!:(s:JourneySnapshot)=>void,a2!:(s:JourneySnapshot)=>void,bad!:(e:Error)=>void;
 vi.mocked(loadJourneySnapshot).mockReset().mockImplementationOnce(()=>new Promise(r=>a1=r)).mockImplementationOnce(()=>new Promise((_,r)=>bad=r)).mockImplementationOnce(()=>new Promise(r=>a2=r)).mockImplementation(()=>new Promise(()=>{}));
 const root=createSmokeRoot();try{
 const render=(ownerSessionId:string)=>root.render(createElement(JourneyOwnerPanel,{ownerSessionId,onClose:vi.fn()}));
 await render("A");await render("B");await render("A");await dispatch(()=>a1(snap("A","STALE-A")));await dispatch(()=>bad(new Error("STALE-B")));expect(root.container.textContent).not.toContain("STALE");
 await dispatch(()=>a2(snap("A","EARLIER-A")));expect(root.container.textContent).toContain("EARLIER-A");const select=root.container.querySelector("select")!;await dispatch(()=>{select.value="EARLIER-A";select.dispatchEvent(new Event("change",{bubbles:true}));});expect(root.container.querySelector("[data-testid=journey-explicit-endpoint]")?.textContent).toContain("hash-EARLIER-A");
 await render("B");expect(root.container.querySelector("[data-testid=journey-explicit-endpoint]")).toBeNull();expect(root.container.textContent).not.toContain("EARLIER-A");
 }finally{await root.unmount();}
});
