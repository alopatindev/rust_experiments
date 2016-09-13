pub struct HuffmanDecoder<R: Read + Seek> {
    input: BitReader<R>,
    code_to_char: HashMap<Code, u8>,
}

impl<R: Read + Seek> HuffmanDecoder<R> {
    pub fn new(input: R) -> Result<Self> {
        let mut result = HuffmanDecoder {
            input: BitReader::new(input),
            code_to_char: HashMap::new(),
        };

        if result.read_header().is_err() {
            let e = Error::new(ErrorKind::InvalidInput, "Failed to read the header");
            Err(e)
        } else {
            Ok(result)
        }
    }

    pub fn decode(&mut self,
                  output: &mut Write,
                  offset_bit: u64,
                  original_length_bits: u64)
                  -> Result<u64> {
        let mut read_bytes = 0;

        if original_length_bits == 0 {
            return Ok(read_bytes);
        }

        let original_length_bytes = original_length_bits / 8;

        try!(self.input.set_position(offset_bit));

        while read_bytes < original_length_bytes {
            match self.read_char() {
                Some(ch) => {
                    try!(output.write_all(&[ch]));
                    read_bytes += 1;
                }
                None => unreachable!(),
            }
        }

        try!(output.flush());

        let read_bits = read_bytes * 8;
        Ok(read_bits)
    }

    pub fn get_input_mut(&mut self) -> &mut R {
        self.input.get_mut()
    }

    pub fn get_reader_mut(&mut self) -> &mut BitReader<R> {
        &mut self.input
    }

    fn read_header(&mut self) -> Result<()> {
        let dict_length = try!(self.input.read_u16()) as usize;
        self.code_to_char.reserve(dict_length);

        for _ in 0..dict_length {
            let code_length = try!(self.input.read_u8());
            let code_data = try!(self.input.read_u8());
            let ch = try!(self.input.read_u8());
            let code = Code {
                length: code_length,
                data: code_data,
            };
            self.code_to_char.insert(code, ch);
        }

        Ok(())
    }

    fn read_char(&mut self) -> Option<u8> {
        let mut code = Code {
            length: 0,
            data: 0,
        };

        while let Ok(data) = self.input.read_bit() {
            if data {
                let shifted_one = 1 << code.length;
                code.data |= shifted_one;
            }

            code.length += 1;

            if let Some(&ch) = self.code_to_char.get(&code) {
                return Some(ch);
            }
        }

        None
    }
}
