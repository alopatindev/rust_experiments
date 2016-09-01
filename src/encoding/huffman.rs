use encoding::bitreader::BitReader;
use encoding::bitwriter::BitWriter;
use std::collections::{HashSet, HashMap};
use std::io::{Read, Result, Seek, Write};
use structs::binary_tree::BinaryTree;

#[derive(Clone, PartialEq, Debug)]
pub struct NodeData {
    chars: HashSet<u8>,
    weight: usize,
}

pub type Tree = BinaryTree<NodeData>;

#[derive(PartialEq, Eq, Hash)]
pub struct Code {
    length: u8,
    data: u8,
}

pub type CodesToChars = HashMap<Code, u8>;
pub type CharsToCodes = HashMap<u8, Code>;

pub fn compress<R, W>(input: &mut BitReader<R>, output: &mut BitWriter<W>) -> Result<usize>
    where R: Read + Seek,
          W: Write
{
    let tree = compression::build_tree(input);
    let chars_to_codes = compression::build_dictionary(&tree);
    try!(compression::write_dictionary(output, &chars_to_codes));
    compression::write_compressed(input, output, &chars_to_codes)
}

pub fn decompress<R>(input: &mut BitReader<R>, output: &mut Write) -> Result<usize>
    where R: Read
{
    let codes_to_chars = try!(decompression::read_dictionary(input));
    decompression::read_compressed(input, output, &codes_to_chars)
}

mod compression {
    use encoding::bitreader::BitReader;
    use encoding::bitwriter::BitWriter;
    use std::collections::{HashMap, HashSet};
    use std::io::{Read, Result, Seek, SeekFrom, Write};
    use structs::binary_tree::BinaryTree;
    use structs::bitset::BitSet;
    use super::*;

    pub fn write_dictionary<W>(output: &mut BitWriter<W>,
                               chars_to_codes: &CharsToCodes)
                               -> Result<()>
        where W: Write
    {
        let max_index = (chars_to_codes.len() - 1) as u8;
        try!(output.write_byte(max_index));
        for (&ch, code) in chars_to_codes {
            try!(output.write_byte(code.length));
            try!(output.write_byte(code.data));
            try!(output.write_byte(ch));
        }

        Ok(())
    }

    pub fn write_compressed<R, W>(input: &mut BitReader<R>,
                                  output: &mut BitWriter<W>,
                                  chars_to_codes: &CharsToCodes)
                                  -> Result<usize>
        where R: Read + Seek,
              W: Write
    {
        try!(input.get_mut().seek(SeekFrom::Start(0)));

        let mut bits_written = 0;
        while let Ok(buffer) = input.read_byte() {
            let code = chars_to_codes.get(&buffer).unwrap();
            for i in 0..code.length {
                let bit = 1 << i;
                let data = (code.data & bit) > 0;
                try!(output.write_bit(data));
                bits_written += 1;
            }
        }

        Ok(bits_written)
    }

    pub fn compute_leaves<R>(input: &mut BitReader<R>) -> Vec<Tree>
        where R: Read + Seek
    {
        let mut char_to_weight: HashMap<u8, usize> = HashMap::new();

        while let Ok(buffer) = input.read_byte() {
            char_to_weight.entry(buffer).or_insert(0);
            char_to_weight.get_mut(&buffer).map(|mut w| *w += 1);
        }

        let mut result = Vec::with_capacity(char_to_weight.len());
        for (&ch, &weight) in &char_to_weight {
            let chars = hashset!{ch};
            let data: NodeData = NodeData {
                chars: chars,
                weight: weight,
            };
            result.push(BinaryTree::new_leaf(data));
        }

        result
    }

    pub fn build_next_level(level: &[Tree], next_level: &mut Vec<Tree>) {
        let n = level.len();
        let mut i = 0;
        while i < n {
            let last_node_in_level = i == n - 1;
            let new_parent_has_same_weight = match next_level.last() {
                Some(tree) => tree.data().unwrap().weight <= level[i].data().unwrap().weight,
                None => false,
            };
            if last_node_in_level || new_parent_has_same_weight {
                let parent = new_parent(next_level.last().unwrap(), &level[i]);
                next_level.pop();
                next_level.push(parent);
                i += 1;
            } else {
                let parent = new_parent(&level[i], &level[i + 1]);
                next_level.push(parent);
                i += 2;
            }
        }
    }

    pub fn new_parent(left: &Tree, right: &Tree) -> Tree {
        let left_chars = &left.data().unwrap().chars;
        let right_chars = &right.data().unwrap().chars;

        let chars = left_chars.union(right_chars).cloned().collect::<HashSet<u8>>();
        let weight = left.data().unwrap().weight + right.data().unwrap().weight;

        let data = NodeData {
            chars: chars,
            weight: weight,
        };
        Tree::new(data, left, right)
    }

    pub fn build_tree<R>(chars: &mut BitReader<R>) -> Tree
        where R: Read + Seek
    {
        let mut leaves = compute_leaves(chars);
        leaves.sort_by_key(|tree| tree.data().unwrap().weight);

        let mut level = leaves;
        let mut next_level = Vec::with_capacity(level.len() / 2 + 1);

        loop {
            let found_root = next_level.is_empty() && level.len() == 1;
            if found_root {
                break;
            } else {
                build_next_level(&level, &mut next_level);
                level = next_level;
                next_level = vec![];
            }
        }

        level[0].clone()
    }

    pub fn compute_code(ch: u8, tree: &Tree) -> Code {
        let mut tree = tree.clone();

        let mut code = BitSet::new();
        let mut length = 0;

        loop {
            if tree.left_data().is_some() && tree.left_data().unwrap().chars.contains(&ch) {
                tree = tree.left();
            } else if tree.right_data().is_some() &&
                      tree.right_data().unwrap().chars.contains(&ch) {
                code.insert(length);
                tree = tree.right();
            } else {
                break;
            }
            length += 1;
        }

        assert!(tree.is_leaf());

        Code {
            length: length as u8,
            data: code.as_slice()[0] as u8,
        }
    }

    pub fn build_dictionary(tree: &Tree) -> CharsToCodes {
        let mut result = HashMap::new();
        for &ch in &tree.data().unwrap().chars {
            let code = compute_code(ch, tree);
            result.insert(ch, code);
        }
        result
    }
}

mod decompression {
    use encoding::bitreader::BitReader;
    use std::io::{Read, Result, Write};
    use super::*;

    pub fn read_dictionary<R>(input: &mut BitReader<R>) -> Result<CodesToChars>
        where R: Read
    {
        let max_index = try!(input.read_byte());
        let len = max_index + 1;
        let len = len as usize;
        let mut result = CodesToChars::with_capacity(len);

        for _ in 0..len {
            let code_length = try!(input.read_byte());
            let code_data = try!(input.read_byte());
            let ch = try!(input.read_byte());
            let code = Code {
                length: code_length,
                data: code_data,
            };
            result.insert(code, ch);
        }

        Ok(result)
    }

    pub fn read_compressed<R>(input: &mut BitReader<R>,
                              output: &mut Write,
                              codes_to_chars: &CodesToChars)
                              -> Result<usize>
        where R: Read
    {
        let mut read_bytes = 0;

        while let Some(ch) = read_char(input, codes_to_chars) {
            println!("read_compressed ch={}", ch);
            try!(output.write(&[ch]));
            read_bytes += 1;
        }

        let read_bits = read_bytes * 8;
        Ok(read_bits)
    }

    fn read_char<R>(input: &mut BitReader<R>, codes_to_chars: &CodesToChars) -> Option<u8>
        where R: Read
    {
        let mut code = Code {
            length: 0,
            data: 0,
        };

        while let Ok(data) = input.read_bit() {
            if data {
                let bit = 1 << code.length;
                code.data |= bit;
            }
            code.length += 1;
            if let Some(&ch) = codes_to_chars.get(&code) {
                return Some(ch);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use encoding::bitreader::BitReader;
    use encoding::bitwriter::BitWriter;
    use std::io::{Cursor, BufWriter, Write};
    use super::*;

    #[test]
    fn simple() {
        simple_assert("mississippi river");
    }

    // TODO: quickcheck

    fn simple_assert(text: &str) {
        let input_slice = text.as_bytes();
        let mut input = BitReader::new(Cursor::new(input_slice));

        let output: Vec<u8> = vec![];
        let mut output = BitWriter::new(Cursor::new(output));

        let compressed_length = compress(&mut input, &mut output).unwrap();
        // assert_eq!(46, compressed_length);

        let decompressed: Vec<u8> = vec![];
        let mut decompressed = BufWriter::new(decompressed);

        let mut compressed: BitReader<&[u8]> = BitReader::new(&output.get_ref().get_ref()[..]);
        let decompressed_length = decompress(&mut compressed, decompressed.by_ref()).unwrap();

        assert!(compressed_length < decompressed_length);
        assert_eq!(decompressed_length, input_slice.len() * 8);
        assert_eq!(input_slice, &decompressed.get_ref()[..]);
    }

    #[test]
    fn compute_leaves() {
        let text = "mississippi river";
        let input_slice = text.as_bytes();
        let input = Cursor::new(input_slice);
        let mut input = BitReader::new(input);

        let expected = vec![(' ', 1), ('e', 1), ('i', 5), ('m', 1), ('p', 2), ('r', 2), ('s', 4),
                            ('v', 1)];
        let expected = expected.into_iter()
            .map(|(ch, weight)| {
                NodeData {
                    chars: hashset!{ch as u8},
                    weight: weight,
                }
            })
            .collect::<Vec<NodeData>>();

        let mut result: Vec<NodeData> = super::compression::compute_leaves(&mut input)
            .iter()
            .map(|tree| tree.data().unwrap().clone())
            .collect::<Vec<NodeData>>();
        result.sort_by_key(|node| *node.chars.iter().next().unwrap());

        assert_eq!(expected, result);
    }

    #[test]
    fn build_tree() {
        use std::collections::HashSet;
        let text = "mississippi river";
        let input_slice = text.as_bytes();
        let input = Cursor::new(input_slice);
        let mut input = BitReader::new(input);
        let tree = super::compression::build_tree(&mut input);

        let assert_weight = |expect: usize, tree: &Tree| {
            assert_eq!(expect, tree.data().unwrap().weight);
        };

        let mut all_chars = HashSet::with_capacity(input_slice.len());
        for &i in input_slice {
            all_chars.insert(i);
        }

        assert_eq!(all_chars, tree.data().unwrap().chars);
        assert_weight(17, &tree);
        assert_weight(6, &tree.left());
        assert_weight(2, &tree.left().left());
        assert_weight(1, &tree.left().left().left());
        assert!(tree.left().left().left().is_leaf());
        assert_weight(1, &tree.left().left().right());
        assert!(tree.left().left().right().is_leaf());
        assert_weight(4, &tree.left().right());
        assert_weight(2, &tree.left().right().left());
        assert_weight(1, &tree.left().right().left().left());
        assert!(tree.left().right().left().left().is_leaf());
        assert_weight(1, &tree.left().right().left().right());
        assert!(tree.left().right().left().right().is_leaf());
        assert_weight(2, &tree.left().right().right());
        assert_weight(11, &tree.right());
        assert_weight(6, &tree.right().left());
        assert_weight(2, &tree.right().left().left());
        assert!(tree.right().left().left().is_leaf());
        assert_weight(4, &tree.right().left().right());
        assert!(tree.right().left().right().is_leaf());
        assert_weight(5, &tree.right().right());
    }
}
