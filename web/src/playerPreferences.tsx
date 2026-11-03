import {createContext,useContext,useState,type ReactNode} from 'react'
type Settings={encoder:string;height:number;transcode:boolean;subtitle:{label:string;format:string;sourceId?:string;path?:string;track?:string}|null}
const defaults:Settings={encoder:'auto',height:720,transcode:false,subtitle:null}
const Context=createContext<{settings:Settings;update:(patch:Partial<Settings>)=>void}|null>(null)
export const usePlayerPreferences=()=>useContext(Context)
export function PlayerPreferences({userId,children}:{userId?:string;children:ReactNode}){
 const key=userId?`grimoire:player:v1:${userId}`:null
 const [settings,setSettings]=useState<Settings>(()=>{try{const v=JSON.parse(key?localStorage.getItem(key)??'null':'null');return {...defaults,...(v&&typeof v==='object'?{encoder:['auto','cpu','amd','nvidia','amf'].includes(v.encoder)?v.encoder:'auto',height:v.height===1080?1080:720,transcode:v.transcode===true,subtitle:v.subtitle&&typeof v.subtitle.label==='string'&&['ass','vtt'].includes(v.subtitle.format)?{label:v.subtitle.label,format:v.subtitle.format,...(typeof v.subtitle.sourceId==='string'&&typeof v.subtitle.path==='string'&&typeof v.subtitle.track==='string'?{sourceId:v.subtitle.sourceId,path:v.subtitle.path,track:v.subtitle.track}:{})}:null}:{})}}catch{return defaults}})
 const update=(patch:Partial<Settings>)=>setSettings(old=>{const next={...old,...patch};try{if(key)localStorage.setItem(key,JSON.stringify(next))}catch{/* Private browsing/storage quotas must not prevent playback. */}return next})
 return <Context.Provider value={{settings,update}}>{children}</Context.Provider>
}
