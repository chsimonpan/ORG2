import {describe,it,expect,vi,beforeEach} from "vitest";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
import {invoke} from "@tauri-apps/api/core";
import {loadJourneyTitles,generateJourneyTitles} from "./nativeAdapter";
const ok=(o:string)=>({ownerSessionId:o,titles:[{nodeId:"n",title:"标题",status:"ok",lastError:null,model:"m"}],pending:0});
describe("journey titles adapter",()=>{
 beforeEach(()=>vi.mocked(invoke).mockReset());
 it("reads titles through the snapshot command",async()=>{vi.mocked(invoke).mockResolvedValue(ok("A") as never);const r=await loadJourneyTitles("A");expect(invoke).toHaveBeenCalledWith("journey_titles_snapshot",{ownerSessionId:"A"});expect(r.titles[0].title).toBe("标题");});
 it("generates only through the explicit generate command",async()=>{vi.mocked(invoke).mockResolvedValue(ok("A") as never);await generateJourneyTitles("A");expect(invoke).toHaveBeenCalledWith("journey_titles_generate",{ownerSessionId:"A"});});
 it("rejects a response for a different owner",async()=>{vi.mocked(invoke).mockResolvedValue(ok("B") as never);await expect(loadJourneyTitles("A")).rejects.toThrow(/mismatch/);});
 it("rejects cli: and empty owners without calling native",async()=>{await expect(loadJourneyTitles("cli:x")).rejects.toThrow();await expect(generateJourneyTitles("")).rejects.toThrow();expect(invoke).not.toHaveBeenCalled();});
});
