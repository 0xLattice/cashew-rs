import Foundation
import cashew
import Crypto
struct Record: Codable {let name:String;let value:Int;let bytes:Data}
func emit<T:Codable>(_ name:String,_ value:T) throws { print(name+"\t"+(try DagCBOR.encode(value)).map{String(format:"%02x",$0)}.joined()) }
try emit("int", UInt64.max)
try emit("negative", Int64.min)
try emit("float", 1.5)
try emit("record", Record(name:"test",value:23,bytes:Data([0,1,255])))
for n in 0...32 {
 var dictionary=MerkleDictionaryImpl<Int>()
 for i in 0..<n {dictionary=try dictionary.inserting(key:["cat","car","cab","c","dog","🐱","🐱x","é","éx"][i%9]+String(i/9),value:i)}
 try emit("dictionary-\(n)",dictionary)
 let header=try HeaderImpl(node:dictionary)
 print("cid-\(n)\t"+header.rawCID)
}
let encrypted=try EncryptionHelper.encrypt(data:Data([0,1,2,3]),key:SymmetricKey(data:Data(repeating:7,count:32)),nonce:AES.GCM.Nonce(data:Data(repeating:8,count:12)))
print("encrypted\t"+encrypted.map{String(format:"%02x",$0)}.joined())
