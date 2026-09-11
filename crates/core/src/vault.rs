use age::secrecy::SecretString;
use anyhow::{bail, Result};
use std::{collections::BTreeMap, io::{Read, Write}, path::Path};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct Vault { keys: BTreeMap<String, Zeroizing<String>> }
impl Vault {
    pub fn set(&mut self,source:String,key:String) { self.keys.insert(source,Zeroizing::new(key)); }
    pub fn get(&self,source:&str)->Option<Zeroizing<String>> {self.keys.get(source).map(|v|Zeroizing::new(v.to_string()))}
    pub fn lock(&mut self){self.keys.clear();}
    pub fn save(&self,path:&Path,passphrase:Zeroizing<String>)->Result<()> {
        if passphrase.len()<12 {bail!("passphrase must contain at least 12 characters");}
        let values:BTreeMap<_,_>=self.keys.iter().map(|(k,v)|(k.as_str(),v.as_str())).collect();
        let plaintext=Zeroizing::new(serde_json::to_vec(&values)?);
        let encryptor=age::Encryptor::with_user_passphrase(SecretString::from(passphrase.to_string()));
        let mut ciphertext=Vec::new();let mut writer=encryptor.wrap_output(&mut ciphertext)?;writer.write_all(&plaintext)?;writer.finish()?;
        let mut file=tempfile::NamedTempFile::new_in(path.parent().ok_or_else(||anyhow::anyhow!("vault needs a directory"))?)?;
        file.write_all(&ciphertext)?;file.as_file().sync_all()?;
        file.persist(path)?;Ok(())
    }
    pub fn unlock(&mut self,path:&Path,passphrase:Zeroizing<String>)->Result<()> {
        if path.metadata()?.len()>1024*1024 {bail!("vault too large");}
        let ciphertext=std::fs::read(path)?;
        let decryptor=age::Decryptor::new(&ciphertext[..])?;
        let identity=age::scrypt::Identity::new(SecretString::from(passphrase.to_string()));
        let mut reader=decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;
        let mut plaintext=Zeroizing::new(Vec::new());reader.read_to_end(&mut plaintext)?;
        let mut values:BTreeMap<String,String>=serde_json::from_slice(&plaintext)?;
        self.keys.clear();for(k,v)in std::mem::take(&mut values){self.set(k,v);}Ok(())
    }
}
