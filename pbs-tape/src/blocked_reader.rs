use std::io::Read;

use crate::{
    TapeRead,
    BlockRead,
    BlockReadError,
    PROXMOX_TAPE_BLOCK_HEADER_MAGIC_1_0,
    BlockHeader,
    BlockHeaderFlags,
};

/// Read a block stream generated by 'BlockWriter'.
///
/// This class implements 'TapeRead'. It always read whole blocks from
/// the underlying reader, and does additional error checks:
///
/// - check magic number (detect streams not written by 'BlockWriter')
/// - check block size
/// - check block sequence numbers
///
/// The reader consumes the EOF mark after the data stream (if read to
/// the end of the stream).
pub struct BlockedReader<R> {
    reader: R,
    buffer: Box<BlockHeader>,
    seq_nr: u32,
    found_end_marker: bool,
    incomplete: bool,
    got_eod: bool,
    read_error: bool,
    read_pos: usize,
}

impl <R: BlockRead> BlockedReader<R> {

    /// Create a new BlockedReader instance.
    ///
    /// This tries to read the first block. Please inspect the error
    /// to detect EOF and EOT.
    pub fn open(mut reader: R) -> Result<Self, BlockReadError> {

        let mut buffer = BlockHeader::new();

        Self::read_block_frame(&mut buffer, &mut reader)?;

        let (_size, found_end_marker) = Self::check_buffer(&buffer, 0)?;

        let mut incomplete = false;
        let mut got_eod = false;

        if found_end_marker {
            incomplete = buffer.flags.contains(BlockHeaderFlags::INCOMPLETE);
            Self::consume_eof_marker(&mut reader)?;
            got_eod = true;
        }

        Ok(Self {
            reader,
            buffer,
            found_end_marker,
            incomplete,
            got_eod,
            seq_nr: 1,
            read_error: false,
            read_pos: 0,
        })
    }

    fn check_buffer(buffer: &BlockHeader, seq_nr: u32) -> Result<(usize, bool), std::io::Error> {

        if buffer.magic != PROXMOX_TAPE_BLOCK_HEADER_MAGIC_1_0 {
            proxmox_lang::io_bail!("detected tape block with wrong magic number - not written by proxmox tape");
        }

        if seq_nr != buffer.seq_nr() {
            proxmox_lang::io_bail!(
                "detected tape block with wrong sequence number ({} != {})",
                seq_nr, buffer.seq_nr())
        }

        let size = buffer.size();
        let found_end_marker = buffer.flags.contains(BlockHeaderFlags::END_OF_STREAM);

        if size > buffer.payload.len() {
            proxmox_lang::io_bail!("detected tape block with wrong payload size ({} > {}", size, buffer.payload.len());
        } else if size == 0 && !found_end_marker {
            proxmox_lang::io_bail!("detected tape block with zero payload size");
        }


        Ok((size, found_end_marker))
    }

    fn read_block_frame(buffer: &mut BlockHeader, reader: &mut R) -> Result<(), BlockReadError> {

        let data = unsafe {
            std::slice::from_raw_parts_mut(
                (buffer as *mut BlockHeader) as *mut u8,
                BlockHeader::SIZE,
            )
        };

        let bytes = reader.read_block(data)?;

        if bytes != BlockHeader::SIZE {
            return Err(proxmox_lang::io_format_err!("got wrong block size").into());
        }

        Ok(())
    }

    fn consume_eof_marker(reader: &mut R) -> Result<(), std::io::Error> {
        let mut tmp_buf = [0u8; 512]; // use a small buffer for testing EOF
        match reader.read_block(&mut tmp_buf) {
            Ok(_) => {
                proxmox_lang::io_bail!("detected tape block after block-stream end marker");
            }
            Err(BlockReadError::EndOfFile) => {
                Ok(())
            }
            Err(BlockReadError::EndOfStream) => {
                proxmox_lang::io_bail!("got unexpected end of tape");
            }
            Err(BlockReadError::Error(err)) => {
                Err(err)
            }
        }
    }

    fn read_block(&mut self, check_end_marker: bool) -> Result<usize, std::io::Error> {

        match Self::read_block_frame(&mut self.buffer, &mut self.reader) {
            Ok(()) => { /* ok */ }
            Err(BlockReadError::EndOfFile) => {
                self.got_eod = true;
                self.read_pos = self.buffer.payload.len();
                if !self.found_end_marker && check_end_marker {
                    proxmox_lang::io_bail!("detected tape stream without end marker");
                }
                return Ok(0); // EOD
            }
            Err(BlockReadError::EndOfStream) => {
                proxmox_lang::io_bail!("got unexpected end of tape");
            }
            Err(BlockReadError::Error(err)) => {
                return Err(err);
            }
        }

        let (size, found_end_marker) = Self::check_buffer(&self.buffer, self.seq_nr)?;
        self.seq_nr += 1;

        if found_end_marker { // consume EOF mark
            self.found_end_marker = true;
            self.incomplete = self.buffer.flags.contains(BlockHeaderFlags::INCOMPLETE);
            Self::consume_eof_marker(&mut self.reader)?;
            self.got_eod = true;
        }

        self.read_pos = 0;

        Ok(size)
    }
}

impl <R: BlockRead> TapeRead for BlockedReader<R> {

    fn is_incomplete(&self) -> Result<bool, std::io::Error> {
        if !self.got_eod {
            proxmox_lang::io_bail!("is_incomplete failed: EOD not reached");
        }
        if !self.found_end_marker {
            proxmox_lang::io_bail!("is_incomplete failed: no end marker found");
        }

        Ok(self.incomplete)
    }

    fn has_end_marker(&self) -> Result<bool, std::io::Error> {
        if !self.got_eod {
            proxmox_lang::io_bail!("has_end_marker failed: EOD not reached");
        }

        Ok(self.found_end_marker)
    }

    // like ReadExt::skip_to_end(), but does not raise an error if the
    // stream has no end marker.
    fn skip_data(&mut self) -> Result<usize, std::io::Error> {
        let mut bytes = 0;
        let buffer_size = self.buffer.size();
        let rest = (buffer_size as isize) - (self.read_pos as isize);
        if rest > 0 {
            bytes = rest as usize;
        }
        loop {
            if self.got_eod {
                return Ok(bytes);
            }
            bytes += self.read_block(false)?;
        }
    }
}

impl <R: BlockRead> Read for BlockedReader<R> {

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, std::io::Error> {

         if self.read_error {
            proxmox_lang::io_bail!("detected read after error - internal error");
        }

        let mut buffer_size = self.buffer.size();
        let mut rest = (buffer_size as isize) - (self.read_pos as isize);

        if rest <= 0 && !self.got_eod { // try to refill buffer
            buffer_size = match self.read_block(true) {
                Ok(len) => len,
                err => {
                    self.read_error = true;
                    return err;
                }
            };
            rest = buffer_size as isize;
        }

        if rest <= 0 {
            Ok(0)
        } else {
            let copy_len = if (buffer.len() as isize) < rest {
                buffer.len()
            } else {
                rest as usize
            };
            buffer[..copy_len].copy_from_slice(
                &self.buffer.payload[self.read_pos..(self.read_pos + copy_len)]);
            self.read_pos += copy_len;
            Ok(copy_len)
        }
    }
}

#[cfg(test)]
mod test {
    use std::io::Read;
    use anyhow::{bail, Error};
    use crate::{
        TapeWrite,
        BlockReadError,
        EmulateTapeReader,
        EmulateTapeWriter,
        PROXMOX_TAPE_BLOCK_SIZE,
        BlockedReader,
        BlockedWriter,
    };

    fn write_and_verify(data: &[u8]) -> Result<(), Error> {

        let mut tape_data = Vec::new();

        {
            let writer = EmulateTapeWriter::new(&mut tape_data, 1024*1024*10);
            let mut writer = BlockedWriter::new(writer);

            writer.write_all(data)?;

            writer.finish(false)?;
        }

        assert_eq!(
            tape_data.len(),
            ((data.len() + PROXMOX_TAPE_BLOCK_SIZE)/PROXMOX_TAPE_BLOCK_SIZE)
                *PROXMOX_TAPE_BLOCK_SIZE
        );

        let reader = &mut &tape_data[..];
        let reader = EmulateTapeReader::new(reader);
        let mut reader = BlockedReader::open(reader)?;

        let mut read_data = Vec::with_capacity(PROXMOX_TAPE_BLOCK_SIZE);
        reader.read_to_end(&mut read_data)?;

        assert_eq!(data.len(), read_data.len());

        assert_eq!(data, &read_data[..]);

        Ok(())
    }

    #[test]
    fn empty_stream() -> Result<(), Error> {
        write_and_verify(b"")
    }

    #[test]
    fn small_data() -> Result<(), Error> {
        write_and_verify(b"ABC")
    }

    #[test]
    fn large_data() -> Result<(), Error> {
        let data = proxmox_sys::linux::random_data(1024*1024*5)?;
        write_and_verify(&data)
    }

    #[test]
    fn no_data() -> Result<(), Error> {
        let tape_data = Vec::new();
        let reader = &mut &tape_data[..];
        let reader = EmulateTapeReader::new(reader);
        match BlockedReader::open(reader) {
            Err(BlockReadError::EndOfFile) => { /* OK */ },
            _ => bail!("expected EOF"),
        }

        Ok(())
    }

    #[test]
    fn no_end_marker() -> Result<(), Error> {
        let mut tape_data = Vec::new();
        {
            let writer = EmulateTapeWriter::new(&mut tape_data, 1024*1024);
            let mut writer = BlockedWriter::new(writer);
            // write at least one block
            let data = proxmox_sys::linux::random_data(PROXMOX_TAPE_BLOCK_SIZE)?;
            writer.write_all(&data)?;
            // but do not call finish here
        }

        let reader = &mut &tape_data[..];
        let reader = EmulateTapeReader::new(reader);
        let mut reader = BlockedReader::open(reader)?;

        let mut data = Vec::with_capacity(PROXMOX_TAPE_BLOCK_SIZE);
        assert!(reader.read_to_end(&mut data).is_err());

        Ok(())
    }

    #[test]
    fn small_read_buffer() -> Result<(), Error> {
        let mut tape_data = Vec::new();

        {
            let writer = EmulateTapeWriter::new(&mut tape_data, 1024*1024);
            let mut writer = BlockedWriter::new(writer);

            writer.write_all(b"ABC")?;

            writer.finish(false)?;
        }

        let reader = &mut &tape_data[..];
        let reader = EmulateTapeReader::new(reader);
        let mut reader = BlockedReader::open(reader)?;

        let mut buf = [0u8; 1];
        assert_eq!(reader.read(&mut buf)?, 1, "wrong byte count");
        assert_eq!(&buf, b"A");
        assert_eq!(reader.read(&mut buf)?, 1, "wrong byte count");
        assert_eq!(&buf, b"B");
        assert_eq!(reader.read(&mut buf)?, 1, "wrong byte count");
        assert_eq!(&buf, b"C");
        assert_eq!(reader.read(&mut buf)?, 0, "wrong byte count");
        assert_eq!(reader.read(&mut buf)?, 0, "wrong byte count");

        Ok(())
    }
}
