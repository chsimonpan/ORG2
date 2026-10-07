import type {DurableAnchor,JourneySnapshot} from './types';
export interface RequestToken {owner:string;scopeRevision:number;requestId:number;}
export class JourneyScope {
 private owner='';private revision=0;private request=0;private mounted=true;
 private accepted=new Map<string,JourneySnapshot>();private selection=new Map<string,DurableAnchor>();
 switchOwner(owner:string){this.owner=owner;this.revision++;this.request++;}
 begin():RequestToken{return {owner:this.owner,scopeRevision:this.revision,requestId:++this.request};}
 current(t:RequestToken){return this.mounted&&t.owner===this.owner&&t.scopeRevision===this.revision&&t.requestId===this.request;}
 accept(t:RequestToken,s:JourneySnapshot){if(!this.current(t)||s.ownerSessionId!==t.owner||s.revision<(this.accepted.get(t.owner)?.revision??0))return false;this.accepted.set(t.owner,s);return true;}
 visible(){return this.accepted.get(this.owner);}
 select(anchor:DurableAnchor){if(anchor.ownerSessionId!==this.owner)throw Error('owner mismatch');this.selection.set(this.owner,anchor);}
 selected(){return this.selection.get(this.owner);}
 dispose(){this.mounted=false;this.request++;}
}
export function validateAnchor(a:DurableAnchor){if(!a.ownerSessionId||!a.sourceSessionId||!a.branchId||!a.durableMessageId||!a.boundaryHash||!Number.isSafeInteger(a.boundaryVersion)||a.boundaryVersion<0)throw Error('Invalid durable anchor');return a;}
