export type ReaderKind = 'Novel' | 'Comic' | 'Manga' | 'Manhua'
export type ReaderSettings = {fit:'original'|'width'|'height'|'both';layout:'single'|'double'|'strip';zoom:number;fontSize:number;theme:'paper'|'night';direction:'ltr'|'rtl'}
export const readerDefaults = (kind:ReaderKind):ReaderSettings => ({fit:kind==='Manhua'?'width':'both',layout:kind==='Manhua'?'strip':'single',zoom:100,fontSize:19,theme:'paper',direction:kind==='Manga'?'rtl':'ltr'})
export function readSettings(key:string,kind:ReaderKind):ReaderSettings {
  const defaults=readerDefaults(kind)
  try {
    const value=JSON.parse(localStorage.getItem(`${key}:${kind}`)??'null')
    if(!value||typeof value!=='object')return defaults
    return {fit:['original','width','height','both'].includes(value.fit)?value.fit:defaults.fit,layout:['single','double','strip'].includes(value.layout)?value.layout:defaults.layout,zoom:Number.isFinite(value.zoom)?Math.max(25,Math.min(400,value.zoom)):100,fontSize:Number.isFinite(value.fontSize)?Math.max(14,Math.min(30,value.fontSize)):19,theme:value.theme==='night'?'night':'paper',direction:value.direction==='rtl'?'rtl':'ltr'}
  }catch{return defaults}
}
export const isSpread = (width:number,height:number) => width>height*1.2
export function spreadMembers(index:number,count:number,wide:Record<number,boolean>,layout:string){return layout==='double'&&index+1<count&&wide[index]===false&&wide[index+1]===false?[index,index+1]:[index]}
