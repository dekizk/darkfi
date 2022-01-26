import copy, utils
from block import Block
from blockchain import Blockchain
from vote import Vote

class Node:
	''' This class represents a protocol node.
		Each node is numbered and has a secret-public keys pair, to sign messages.
		Nodes hold a set of Blockchains(some of which are not notarized) 
		and a set of unconfirmed pending transactions. 
		All nodes have syncronized clocks, using GST approach.'''
	def __init__(self, id, clock, password, init_block):
		self.id = id
		self.clock = clock # Clock syncronization to be implemented.
		self.password = password
		self.private_key, self.public_key = utils.generate_keys(self.password)
		self.blockchain = Blockchain(init_block)
		self.unconfirmed_transactions = []
	
	def __repr__(self):
		return "Node=[id={0}, clock={1}, password={2}, private_key={3}, public_key={4}, blockchain={5}, unconfirmed_transactions={6}".format(self.id, self.clock, self.password, self.private_key, self.public_key, self.blockchain, self.unconfirmed_transactions)
	
	def output(self):
		return self.blockchain
	
	def receive_transaction(self, transaction):
		# Additional validity rules must be defined by the protocol for its blockchain data structure.
		self.unconfirmed_transactions.append(transaction)
	
	def broadcast_transaction(self, nodes, transaction):
		for node in nodes:
			node.receive_transaction(transaction)
			
	def propose_block(self, epoch, nodes):
		propozed_block = Block(hash(self.blockchain.blocks[-1]), epoch, self.unconfirmed_transactions)
		for node in nodes:
			node.receive_proposed_block(copy.deepcopy(propozed_block))
	
	def receive_proposed_block(self, round_block):
		self.round_block = round_block
		
	def vote_on_round_block(self, nodes):
		# Node verifies proposed block extends from one of the longest notarized chains that node has seen at the time.
		# Already notarized check.
		if (self.round_block != self.blockchain.blocks[-1]):
			self.blockchain.check_block_validity(self.round_block, self.blockchain.blocks[-1])
		signed_block = utils.sign_message(self.password, self.private_key, self.round_block)
		vote = Vote(signed_block, self.round_block, self.id)
		for node in nodes:
			node.receive_vote(self.public_key, vote, nodes)

	def receive_vote(self, node_public_key, vote, nodes):
		# We verify we haven't received a vote from that node again.
		assert(vote not in self.round_block.votes)
		# When nodes receive votes, they verify them against nodes public key.
		assert(utils.verify_signature(node_public_key, vote.block, vote.vote))
		assert(self.round_block == vote.block)
		# Additional rules must be defined by the protocol for its voting system.
		self.round_block.votes.append(vote)
		# When a node sees 2n/3 votes for a block it notarizes it
		if (self.round_block != self.blockchain.blocks[-1] and len(self.round_block.votes) > (2 * len(nodes) / 3)):
			notarized_block = copy.deepcopy(self.round_block)
			notarized_block.notarized = True
			self.blockchain.add_block(notarized_block)
			# Node removes block transactions from unconfirmed_transactions array
			for transaction in notarized_block.txs:
				self.unconfirmed_transactions.remove(transaction)
			
			