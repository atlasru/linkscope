use anyhow::Result;
use crate::model::Node;
use tantivy::{schema::*, collector::TopDocs, query::{QueryParser, FuzzyTermQuery, BooleanQuery, Occur, Query}, Index, IndexReader, IndexWriter, Term, TantivyDocument};

pub struct Search { index: Index, reader: IndexReader, writer: IndexWriter, id: Field, text: Field }
impl Search {
    pub fn new() -> Result<Self> {
        let mut s = Schema::builder(); let id = s.add_text_field("id", STRING | STORED); let text = s.add_text_field("text", TEXT);
        let index = Index::create_in_ram(s.build());
        let writer = index.writer_with_num_threads(1, 16_000_000)?;
        let reader = index.reader()?;
        Ok(Self { index, reader, writer, id, text })
    }
    pub fn upsert(&mut self, n: &Node) -> Result<()> {
        self.writer.delete_term(Term::from_field_text(self.id, &n.id));
        self.writer.add_document(tantivy::doc!(self.id => n.id.clone(), self.text => serde_json::to_string(n)?))?;
        Ok(())
    }
    pub fn commit(&mut self) -> Result<()> { self.writer.commit()?; self.reader.reload()?; Ok(()) }
    pub fn find(&self, text: &str, limit: usize) -> Result<Vec<String>> {
        if text.trim().is_empty() { return Ok(vec![]); }
        let parser = QueryParser::for_index(&self.index, vec![self.text]);
        let (parsed, _) = parser.parse_query_lenient(text);
        let mut queries: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Should, parsed)];
        for word in text.split_whitespace().take(8) { queries.push((Occur::Should, Box::new(FuzzyTermQuery::new(Term::from_field_text(self.text, &word.to_lowercase()), 1, true)))); }
        let searcher = self.reader.searcher();
        let hits = searcher.search(&BooleanQuery::new(queries), &TopDocs::with_limit(limit.clamp(1, 1000)))?;
        hits.into_iter().map(|(_, addr)| { let d: TantivyDocument = searcher.doc(addr)?; Ok(d.get_first(self.id).and_then(|v| v.as_str()).unwrap_or_default().into()) }).collect()
    }
}
