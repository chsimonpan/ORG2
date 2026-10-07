import {describe,it,expect} from 'vitest';import {buildTimeline,dataSignature,sample,ribbonPath,centerPath} from './visualAlgorithms';
describe('licensed visual timeline algorithm repairs',()=>{
 it.each([[[]],[[10]],[[10,10]]])('finite empty/single timestamps %j',(times)=>{const t=buildTimeline(times);expect(Number.isFinite(t.width)).toBe(true);expect(Number.isFinite(t.scale)).toBe(true);expect(Number.isFinite(t.at(10))).toBe(true);});
 it('rejects nonfinite timestamps rather than inventing chronology',()=>{expect(()=>buildTimeline([NaN])).toThrow();expect(()=>buildTimeline([Infinity])).toThrow();});
 it('samecount titleedit and revision edits invalidate geometry/data cache',()=>{const a={revision:1,nodes:[{id:'node',title:'old',timestamp:10}]};expect(dataSignature(a)).not.toBe(dataSignature({...a,nodes:[{...a.nodes[0],title:'new'}]}));expect(dataSignature(a)).not.toBe(dataSignature({...a,revision:2}));});
 it('retains chronology and marks compressed idle gap',()=>{const t=buildTimeline([0,60000,10000000]);expect(t.at(0)).toBeLessThan(t.at(60000));expect(t.at(60000)).toBeLessThan(t.at(10000000));expect(t.idle).toHaveLength(1);});
 it('actual upstream CatmullRom center+ribbon source produces closed finite geometry',()=>{const pts=sample([{x:0,y:0,w:6},{x:10,y:20,w:12},{x:40,y:8,w:8}]);expect(pts.length).toBeGreaterThan(3);expect(ribbonPath(pts)).toMatch(/^M.*Z$/);expect(centerPath(pts)).not.toMatch(/NaN|Infinity/);});
 it('empty source geometries are valid empty paths',()=>{expect(ribbonPath([])).toBe('');expect(centerPath([])).toBe('');});
});
