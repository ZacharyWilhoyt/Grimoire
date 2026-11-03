import {createContext,useContext,useState,type ReactNode} from 'react'
type Settings={height:number;subtitle:{label:string;format:string;sourceId?:string;path?:string;track?:string}|null}
const defaults:Settings={height:0,subtitle:null}
const Context=createContext<{settings:Settings;update:(patch:Partial<Settings>)=>void}|null>(null)
export const usePlayerPreferences=()=>useContext(Context)
export function PlayerPreferences({userId,children}:{userId?:string;children:ReactNode}){
 const key=userId?`grimoire:player:v1:${userId}`:null
 const [settings,setSettings]=useState<Settings>(()=>{try{const v=JSON.parse(key?localStorage.getItem(key)??'null':'null');return {...defaults,...(v&&typeof v==='object'?{height:[0,480,720,1080,1440,2160].includes(v.height)?v.height:0,subtitle:v.subtitle&&typeof v.subtitle.label==='string'&&['ass','vtt'].includes(v.subtitle.format)?{label:v.subtitle.label,format:v.subtitle.format,...(typeof v.subtitle.sourceId==='string'&&typeof v.subtitle.path==='string'&&typeof v.subtitle.track==='string'?{sourceId:v.subtitle.sourceId,path:v.subtitle.path,track:v.subtitle.track}:{})}:null}:{})}}catch{return defaults}})
 const update=(patch:Partial<Settings>)=>setSettings(old=>{const next={...old,...patch};try{if(key)localStorage.setItem(key,JSON.stringify(next))}catch{/* Private browsing/storage quotas must not prevent playback. */}return next})
 return <Context.Provider value={{settings,update}}>{children}</Context.Provider>
}
