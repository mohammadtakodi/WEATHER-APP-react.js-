use regex::Regex;
use std::cmp;

pub struct SemanticChunker {
    chunk_size: usize,
    overlap: usize,
}

impl SemanticChunker {
    pub fn new(chunk_size: usize, overlap: usize) -> Self {
        Self {
            chunk_size,
            overlap,
        }
    }

    /// Split text into semantic chunks with overlap
    pub fn chunk(&self, text: &str) -> Vec<String> {
        // 1. First, check for code blocks (```...```) and try to keep them intact if possible
        // But for simplicity in this MVP, we will treat code blocks as single units if they fit,
        // or split them if they are too large.
        
        // 2. Split by paragraphs first (double newline)
        let paragraphs: Vec<&str> = text.split("\n\n").collect();
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();

        for paragraph in paragraphs {
            // If adding this paragraph exceeds chunk size, push current chunk and start new one
            if current_chunk.len() + paragraph.len() > self.chunk_size && !current_chunk.is_empty() {
                chunks.push(current_chunk.clone());
                
                // Handle overlap: take the last `overlap` characters from the previous chunk
                // to start the new chunk. Be careful with UTF-8 boundaries.
                let overlap_start = if current_chunk.len() > self.overlap {
                    current_chunk.len() - self.overlap
                } else {
                    0
                };
                
                // Find the nearest space to avoid cutting words in half
                let mut safe_start = overlap_start;
                if safe_start > 0 {
                    if let Some(space_idx) = current_chunk[safe_start..].find(' ') {
                        safe_start += space_idx;
                    }
                }

                let overlap_text = &current_chunk[safe_start..];
                current_chunk = String::from(overlap_text);
                
                // If the paragraph itself is larger than chunk_size, we need to split it further by sentences
                if paragraph.len() > self.chunk_size {
                    let sentences = self.split_sentences(paragraph);
                    for sentence in sentences {
                        if current_chunk.len() + sentence.len() > self.chunk_size {
                            chunks.push(current_chunk.clone());
                            current_chunk = String::new(); // Reset for very long paragraph case
                        }
                        if !current_chunk.is_empty() {
                            current_chunk.push(' ');
                        }
                        current_chunk.push_str(&sentence);
                    }
                } else {
                     if !current_chunk.is_empty() {
                        current_chunk.push_str("\n\n");
                    }
                    current_chunk.push_str(paragraph);
                }
            } else {
                if !current_chunk.is_empty() {
                    current_chunk.push_str("\n\n");
                }
                current_chunk.push_str(paragraph);
            }
        }

        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        chunks
    }

    fn split_sentences(&self, text: &str) -> Vec<String> {
        // Simple sentence splitter: look for .!? followed by space or end of string
        // In production, use a proper NLP tokenizer
        let re = Regex::new(r"(?P<sentence>.*?[.!?])(\s+|$)").unwrap();
        let mut sentences = Vec::new();
        let mut last_end = 0;

        for cap in re.captures_iter(text) {
            let sentence = cap.name("sentence").unwrap().as_str();
            sentences.push(sentence.trim().to_string());
            last_end = cap.get(0).unwrap().end();
        }

        // Add any remaining text
        if last_end < text.len() {
            sentences.push(text[last_end..].trim().to_string());
        }

        sentences
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunking() {
        let chunker = SemanticChunker::new(50, 10);
        let text = "This is sentence one. This is sentence two. This is sentence three.";
        let chunks = chunker.chunk(text);
        assert!(!chunks.is_empty());
        println!("{:?}", chunks);
    }
}
